import fs from 'node:fs';
import path from 'node:path';
import { type Config, paths } from './config.ts';
import { branchExists, git, hasRemote, isDirty, tryGit, worktrees } from './git.ts';

export interface Task {
  id: string;
  repos: string[];
  base: string;
  createdAt: string;
  status: 'active' | 'closed';
}

/** The worktree that permanently holds the staging branch, so no task worktree ever does. */
export const STAGING_WORKTREE = '_staging';

const taskFile = (id: string) => path.join(paths.tasks, `${id}.json`);

export function repoDir(cfg: Config, repo: string): string {
  return path.join(cfg.workspace, repo);
}

export function worktreePath(cfg: Config, repo: string, task: string): string {
  return path.join(cfg.workspace, cfg.worktreeDir.replaceAll('{repo}', repo).replaceAll('{task}', task));
}

export function planPath(cfg: Config, id: string): string {
  return path.join(cfg.workspace, cfg.plansDir, id, 'plan.md');
}

export function loadTask(id: string): Task {
  if (!fs.existsSync(taskFile(id))) throw new Error(`No task ${id}. See \`whet task list\`.`);
  return JSON.parse(fs.readFileSync(taskFile(id), 'utf8'));
}

function saveTask(task: Task): void {
  fs.mkdirSync(paths.tasks, { recursive: true });
  fs.writeFileSync(taskFile(task.id), JSON.stringify(task, null, 2) + '\n');
}

export function listTasks(): Task[] {
  if (!fs.existsSync(paths.tasks)) return [];
  return fs
    .readdirSync(paths.tasks)
    .filter((f) => f.endsWith('.json'))
    .map((f) => JSON.parse(fs.readFileSync(path.join(paths.tasks, f), 'utf8')) as Task);
}

/** origin/<base> when the repo has a remote (fetched first), else the local base. */
function baseRef(dir: string, base: string): string {
  if (!hasRemote(dir)) return base;
  git(dir, ['fetch', 'origin', base]);
  return `origin/${base}`;
}

export function newTask(cfg: Config, id: string, repos: string[], base = cfg.baseBranch): string[] {
  if (fs.existsSync(taskFile(id))) throw new Error(`Task ${id} already exists.`);
  for (const repo of repos) {
    if (!fs.existsSync(path.join(repoDir(cfg, repo), '.git'))) {
      throw new Error(`${repo} is not a git repo in ${cfg.workspace}.`);
    }
  }

  const log: string[] = [];
  for (const repo of repos) {
    const dir = repoDir(cfg, repo);
    const wt = worktreePath(cfg, repo, id);
    if (fs.existsSync(wt)) {
      log.push(`${repo}: worktree already at ${wt}, keeping it`);
      continue;
    }
    if (branchExists(dir, `refs/heads/${id}`)) {
      git(dir, ['worktree', 'add', wt, id]);
      log.push(`${repo}: ${wt} on existing branch ${id}`);
    } else {
      const from = baseRef(dir, base);
      git(dir, ['worktree', 'add', '--no-track', '-b', id, wt, from]);
      log.push(`${repo}: ${wt} on new branch ${id} from ${from}`);
    }
  }

  const plan = planPath(cfg, id);
  if (!fs.existsSync(plan)) {
    fs.mkdirSync(path.dirname(plan), { recursive: true });
    const lines = repos.map((r) => `- ${r}: ${worktreePath(cfg, r, id)}`);
    fs.writeFileSync(plan, `# ${id}\n\n## Worktrees\n${lines.join('\n')}\n\n## Goal\n\n## Design\n`);
    log.push(`plan: ${plan}`);
  }

  saveTask({ id, repos, base, createdAt: new Date().toISOString(), status: 'active' });
  return log;
}

export interface RepoStatus {
  repo: string;
  path: string;
  exists: boolean;
  dirty: boolean;
  ahead: number;
}

export function taskStatus(cfg: Config, task: Task): RepoStatus[] {
  return task.repos.map((repo) => {
    const wt = worktreePath(cfg, repo, task.id);
    if (!fs.existsSync(wt)) return { repo, path: wt, exists: false, dirty: false, ahead: 0 };
    const base = hasRemote(wt) ? `origin/${task.base}` : task.base;
    const ahead = Number(tryGit(wt, ['rev-list', '--count', `${base}..HEAD`]) ?? 0);
    return { repo, path: wt, exists: true, dirty: isDirty(wt), ahead };
  });
}

function samePath(a: string, b: string): boolean {
  const real = (p: string) => (fs.existsSync(p) ? fs.realpathSync(p) : path.resolve(p));
  return real(a) === real(b);
}

/** Makes sure <repo>'s staging worktree exists and holds the staging branch. */
function ensureStagingWorktree(cfg: Config, repo: string, staging: string): string {
  const dir = repoDir(cfg, repo);
  const stagingWt = worktreePath(cfg, repo, STAGING_WORKTREE);
  const holder = worktrees(dir).find((w) => w.branch === staging);
  if (holder && !samePath(holder.path, stagingWt)) {
    throw new Error(
      `${repo}: ${staging} is checked out in ${holder.path}, and whet keeps it in ${stagingWt}.\n` +
        `Commit or stash anything there, then free it with:\n  git -C "${holder.path}" switch --detach`,
    );
  }
  if (!holder) {
    if (hasRemote(dir)) tryGit(dir, ['fetch', 'origin', staging]);
    if (branchExists(dir, `refs/heads/${staging}`)) git(dir, ['worktree', 'add', stagingWt, staging]);
    else if (branchExists(dir, `refs/remotes/origin/${staging}`)) {
      git(dir, ['worktree', 'add', '--track', '-b', staging, stagingWt, `origin/${staging}`]);
    } else throw new Error(`${repo}: no branch ${staging} locally or on origin.`);
  }
  return stagingWt;
}

export interface ShipOptions {
  to?: string;
  push?: boolean;
}

/** Cherry-picks the task's commits that staging doesn't have yet, repo by repo. */
export function shipTask(cfg: Config, task: Task, opts: ShipOptions = {}): string[] {
  const staging = opts.to ?? cfg.stagingBranch;
  if (!staging) throw new Error('No staging branch set. Run `whet config set stagingBranch <branch>` or pass --to.');

  const log: string[] = [];
  for (const repo of task.repos) {
    const dir = repoDir(cfg, repo);
    const stagingWt = ensureStagingWorktree(cfg, repo, staging);
    if (isDirty(stagingWt)) throw new Error(`${repo}: ${stagingWt} has uncommitted changes (an unfinished cherry-pick?).`);
    if (hasRemote(dir)) git(stagingWt, ['pull', '--ff-only', 'origin', staging]);

    const base = hasRemote(dir) ? `origin/${task.base}` : task.base;
    const forkPoint = git(dir, ['merge-base', task.id, base]);
    const picks = git(dir, ['cherry', staging, task.id, forkPoint])
      .split('\n')
      .filter((l) => l.startsWith('+ '))
      .map((l) => l.slice(2));

    if (picks.length === 0) {
      log.push(`${repo}: nothing new for ${staging}`);
      continue;
    }
    for (const sha of picks) {
      if (tryGit(stagingWt, ['cherry-pick', '-x', sha]) === undefined) {
        throw new Error(
          `${repo}: cherry-pick of ${sha.slice(0, 8)} conflicted in ${stagingWt}.\n` +
            `Resolve it there (your agent's /resolving-merge-conflicts works), run \`git cherry-pick --continue\`, ` +
            `then run \`whet task ship ${task.id}\` again. Earlier repos are already done.`,
        );
      }
    }
    if (opts.push !== false && hasRemote(dir)) git(stagingWt, ['push', 'origin', staging]);
    log.push(`${repo}: ${picks.length} commit(s) onto ${staging}${opts.push === false ? ' (not pushed)' : ''}`);
  }
  return log;
}

export function closeTask(cfg: Config, task: Task, force = false): string[] {
  const log: string[] = [];
  for (const s of taskStatus(cfg, task)) {
    if (!s.exists) continue;
    if (s.dirty && !force) throw new Error(`${s.repo}: ${s.path} has uncommitted changes. Commit them, or pass --force.`);
  }
  for (const s of taskStatus(cfg, task)) {
    if (!s.exists) continue;
    git(repoDir(cfg, s.repo), ['worktree', 'remove', ...(force ? ['--force'] : []), s.path]);
    log.push(`${s.repo}: removed ${s.path} (branch ${task.id} kept)`);
  }
  saveTask({ ...task, status: 'closed' });
  return log;
}

export function runPrompt(cfg: Config, task: Task): string {
  const wts = task.repos.map((r) => `- ${r}: ${worktreePath(cfg, r, task.id)}`).join('\n');
  return `We're working on task ${task.id}. The plan is in ${planPath(cfg, task.id)}; read it first.
Work only inside these worktrees:
${wts}
Finish one repo at a time. After each repo, stop and summarize what changed so I can review it before you continue.`;
}
