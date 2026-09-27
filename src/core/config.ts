import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

export const whetHome = process.env.WHET_HOME ?? path.join(os.homedir(), '.whet');

export const paths = {
  home: whetHome,
  config: path.join(whetHome, 'config.json'),
  rules: path.join(whetHome, 'rules.md'),
  inbox: path.join(whetHome, 'inbox.md'),
  tasks: path.join(whetHome, 'tasks'),
};

export type AgentName = 'claude' | 'codex';

export interface Config {
  /** Directory holding the main checkout of every repo. */
  workspace: string;
  /** Where task worktrees go, relative to workspace. {repo} and {task} are replaced. */
  worktreeDir: string;
  /** Where task plans go, relative to workspace. */
  plansDir: string;
  baseBranch: string;
  /** Branch that deploys to staging. Changes per sprint, so it's a setting. */
  stagingBranch?: string;
  agent: AgentName;
}

export const configKeys = ['workspace', 'worktreeDir', 'plansDir', 'baseBranch', 'stagingBranch', 'agent'] as const;

export const defaults: Omit<Config, 'workspace'> = {
  worktreeDir: '{repo}-worktree/{task}',
  plansDir: 'my-plans',
  baseBranch: 'master',
  agent: 'claude',
};

export function expandHome(p: string): string {
  return p === '~' || p.startsWith('~/') ? path.join(os.homedir(), p.slice(1)) : p;
}

export function loadConfig(): Config {
  if (!fs.existsSync(paths.config)) throw new Error('whet is not set up yet. Run `whet init --workspace <dir>` first.');
  const cfg = { ...defaults, ...JSON.parse(fs.readFileSync(paths.config, 'utf8')) } as Config;
  cfg.workspace = expandHome(cfg.workspace);
  return cfg;
}

export function saveConfig(cfg: Config): void {
  fs.mkdirSync(paths.home, { recursive: true });
  fs.writeFileSync(paths.config, JSON.stringify(cfg, null, 2) + '\n');
}
