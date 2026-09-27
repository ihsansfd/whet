/** Runs a command to completion, feeding `input` on stdin, and returns stdout. */
export function capture(cmd: string[], input: string): string {
  const r = Bun.spawnSync(cmd, { stdin: Buffer.from(input), stdout: 'pipe', stderr: 'pipe' });
  if (!r.success) throw new Error(`${cmd[0]} failed: ${r.stderr.toString().trim()}`);
  return r.stdout.toString();
}

/** Runs an interactive command attached to this terminal. */
export function interactive(cmd: string[], cwd: string): void {
  Bun.spawnSync(cmd, { cwd, stdio: ['inherit', 'inherit', 'inherit'] });
}
