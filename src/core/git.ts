export function git(cwd: string, args: string[]): string {
  const r = Bun.spawnSync(['git', ...args], { cwd, stdin: 'ignore', stdout: 'pipe', stderr: 'pipe' });
  if (!r.success) throw new Error(`git ${args.join(' ')} failed in ${cwd}:\n${r.stderr.toString().trim()}`);
  return r.stdout.toString().trim();
}

export function tryGit(cwd: string, args: string[]): string | undefined {
  try {
    return git(cwd, args);
  } catch {
    return undefined;
  }
}

export interface Worktree {
  path: string;
  branch?: string;
}

export function worktrees(repoDir: string): Worktree[] {
  const out: Worktree[] = [];
  for (const block of git(repoDir, ['worktree', 'list', '--porcelain']).split('\n\n')) {
    const wt: Worktree = { path: '' };
    for (const line of block.split('\n')) {
      if (line.startsWith('worktree ')) wt.path = line.slice('worktree '.length);
      if (line.startsWith('branch ')) wt.branch = line.slice('branch refs/heads/'.length);
    }
    if (wt.path) out.push(wt);
  }
  return out;
}

export function isDirty(dir: string): boolean {
  return git(dir, ['status', '--porcelain']).length > 0;
}

export function hasRemote(repoDir: string): boolean {
  return (tryGit(repoDir, ['remote']) ?? '').split('\n').includes('origin');
}

export function branchExists(repoDir: string, ref: string): boolean {
  return tryGit(repoDir, ['rev-parse', '--verify', '--quiet', ref]) !== undefined;
}
