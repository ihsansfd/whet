import { expect, test } from 'bun:test';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const cli = path.join(import.meta.dir, '..', 'src', 'cli.ts');

/** Runs a command; throws with stderr on failure, returns trimmed stdout. */
function run(cmd: string[], opts: { cwd?: string; env?: Record<string, string | undefined> } = {}): string {
  const r = Bun.spawnSync(cmd, { ...opts, stdin: 'ignore', stdout: 'pipe', stderr: 'pipe' });
  if (!r.success) throw new Error(r.stderr.toString());
  return r.stdout.toString().trim();
}

/** A workspace with two repos, each cloned from a bare "origin" with master and a staging branch. */
function setup() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'whet-test-'));
  const env: Record<string, string | undefined> = { ...process.env, WHET_HOME: path.join(root, 'home'), GIT_AUTHOR_NAME: 't', GIT_AUTHOR_EMAIL: 't@t', GIT_COMMITTER_NAME: 't', GIT_COMMITTER_EMAIL: 't@t' };
  const ws = path.join(root, 'ws');
  fs.mkdirSync(ws);
  for (const repo of ['core-service', 'deposit-engine']) {
    const bare = path.join(root, 'remotes', `${repo}.git`);
    run(['git', 'init', '--bare', '-b', 'master', bare], { env });
    const dir = path.join(ws, repo);
    run(['git', 'clone', bare, dir], { env });
    fs.writeFileSync(path.join(dir, 'a.txt'), 'base\n');
    run(['git', 'add', '.'], { cwd: dir, env });
    run(['git', 'commit', '-m', 'base'], { cwd: dir, env });
    run(['git', 'push', 'origin', 'master'], { cwd: dir, env });
    run(['git', 'push', 'origin', 'master:testing/sprint-a'], { cwd: dir, env });
  }
  const whet = (...args: string[]) => run([process.execPath, cli, ...args], { env });
  const commit = (dir: string, file: string, msg: string) => {
    fs.writeFileSync(path.join(dir, file), msg + '\n');
    run(['git', 'add', '.'], { cwd: dir, env });
    run(['git', 'commit', '-m', msg], { cwd: dir, env });
  };
  whet('init', '--workspace', ws, '--staging', 'testing/sprint-a');
  return { root, ws, env, whet, commit };
}

test('task new creates a worktree per repo and a plan', () => {
  const { ws, whet } = setup();
  whet('task', 'new', 'INS-1', 'core-service', 'deposit-engine');
  for (const repo of ['core-service', 'deposit-engine']) {
    const wt = path.join(ws, `${repo}-worktree`, 'INS-1');
    expect(run(['git', 'branch', '--show-current'], { cwd: wt })).toBe('INS-1');
  }
  expect(fs.readFileSync(path.join(ws, 'my-plans', 'INS-1', 'plan.md'), 'utf8')).toMatch(/# INS-1/);
  expect(whet('task', 'list')).toMatch(/INS-1[\s\S]*core-service\s+0 commit\(s\)/);
});

test('ship cherry-picks only new task commits onto staging, and is repeatable', () => {
  const { ws, whet, commit } = setup();
  whet('task', 'new', 'INS-2', 'core-service');
  const wt = path.join(ws, 'core-service-worktree', 'INS-2');
  commit(wt, 'b.txt', 'feature one');

  expect(whet('task', 'ship', 'INS-2')).toMatch(/1 commit\(s\) onto testing\/sprint-a/);
  commit(wt, 'c.txt', 'feature two');
  expect(whet('task', 'ship', 'INS-2')).toMatch(/1 commit\(s\) onto testing\/sprint-a/);
  expect(whet('task', 'ship', 'INS-2')).toMatch(/nothing new/);

  const staging = path.join(ws, 'core-service-worktree', '_staging');
  // Commit subjects may carry a prefix from the user's git hooks, so match loosely.
  expect(run(['git', 'log', '--format=%s', '-2', 'origin/testing/sprint-a'], { cwd: staging })).toMatch(/feature two\n.*feature one$/);
});

test('ship refuses when a task worktree holds the staging branch', () => {
  const { ws, env, whet } = setup();
  const dir = path.join(ws, 'core-service');
  run(['git', 'worktree', 'add', '--track', '-b', 'testing/sprint-a', path.join(ws, 'stray'), 'origin/testing/sprint-a'], { cwd: dir, env });
  whet('task', 'new', 'INS-3', 'core-service');
  expect(() => whet('task', 'ship', 'INS-3')).toThrow(/checked out in .*stray[\s\S]*switch --detach/);
});

test('close removes worktrees but refuses uncommitted work', () => {
  const { ws, whet } = setup();
  whet('task', 'new', 'INS-4', 'core-service');
  const wt = path.join(ws, 'core-service-worktree', 'INS-4');
  fs.writeFileSync(path.join(wt, 'wip.txt'), 'wip');
  expect(() => whet('task', 'close', 'INS-4')).toThrow(/uncommitted changes/);
  whet('task', 'close', 'INS-4', '--force');
  expect(fs.existsSync(wt)).toBe(false);
  expect(whet('task', 'list')).toMatch(/No active tasks/);
});
