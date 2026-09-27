#!/usr/bin/env bun
import fs from 'node:fs';
import { parseArgs } from 'node:util';
import type { Adapter } from './adapters/adapter.ts';
import { claudeAdapter } from './adapters/claude.ts';
import { codexAdapter } from './adapters/codex.ts';
import { type AgentName, type Config, configKeys, defaults, expandHome, loadConfig, paths, saveConfig } from './core/config.ts';
import { pendingCount, readRules, reviewPrompt, RULES_LINE_CAP, runHarvest, startHarvest } from './core/learn.ts';
import { closeTask, listTasks, loadTask, newTask, planPath, runPrompt, shipTask, taskStatus } from './core/tasks.ts';

/** How whet re-invokes itself: the compiled binary alone, or bun plus this script. */
const self = import.meta.path.startsWith('/$bunfs/') ? [process.execPath] : [process.execPath, import.meta.path];
/** How hooks and generated commands call whet: absolute paths, so they work without PATH. */
const cliCommand = self.map((p) => `"${p}"`).join(' ');

const USAGE = `whet: sharpen your AI coding agent

Setup
  whet init --workspace <dir> [--base master] [--staging <branch>] [--agent claude|codex]
  whet install <claude|codex>        wire rules, harvesting and /wrap-up into that tool
  whet config [get <key> | set <key> <value>]

Rules
  whet rules                         show the rules every tool reads
  whet learn review                  turn corrections into rules, with your approval
  whet learn pending                 how many candidate rules are waiting

Tasks
  whet task new <id> <repo...>       worktree per repo + plan, from the base branch
  whet task list
  whet task run <id>                 start your agent on the task's worktrees
  whet task ship <id> [--to <branch>] [--no-push]
                                     cherry-pick new commits onto staging
  whet task close <id> [--force]     remove the worktrees (branches are kept)`;

function adapterFor(name: string): Adapter {
  if (name === 'claude') return claudeAdapter(cliCommand);
  if (name === 'codex') return codexAdapter();
  throw new Error(`Unknown agent "${name}". Supported: claude, codex.`);
}

function print(lines: string[]): void {
  for (const l of lines) console.log(l);
}

function readStdinJson(): Record<string, string> {
  if (process.stdin.isTTY) return {};
  const raw = fs.readFileSync(0, 'utf8').trim();
  return raw ? JSON.parse(raw) : {};
}

function init(flags: Record<string, string | boolean | undefined>): void {
  if (typeof flags.workspace !== 'string') throw new Error('Pass --workspace <dir>: the folder holding your repos.');
  const cfg: Config = {
    ...defaults,
    workspace: expandHome(flags.workspace),
    ...(typeof flags.base === 'string' && { baseBranch: flags.base }),
    ...(typeof flags.staging === 'string' && { stagingBranch: flags.staging }),
    ...(typeof flags.agent === 'string' && { agent: flags.agent as AgentName }),
  };
  saveConfig(cfg);
  fs.mkdirSync(paths.tasks, { recursive: true });
  if (!fs.existsSync(paths.rules)) fs.writeFileSync(paths.rules, '');
  print([`whet home: ${paths.home}`, `workspace: ${cfg.workspace}`, `next: whet install ${cfg.agent}`]);
}

function config(args: string[]): void {
  const cfg = loadConfig();
  const [op, key, value] = args;
  if (!op) return console.log(JSON.stringify(cfg, null, 2));
  if (!configKeys.includes(key as (typeof configKeys)[number])) throw new Error(`Unknown key. Keys: ${configKeys.join(', ')}`);
  if (op === 'get') return console.log(cfg[key as keyof Config] ?? '');
  if (op === 'set' && value !== undefined) {
    saveConfig({ ...cfg, [key]: key === 'workspace' ? expandHome(value) : value });
    return console.log(`${key} = ${value}`);
  }
  throw new Error('Usage: whet config [get <key> | set <key> <value>]');
}

function learn(sub: string, flags: Record<string, string | boolean | undefined>): void {
  const from = typeof flags.from === 'string' ? flags.from : undefined;
  switch (sub) {
    case 'harvest': {
      // Session-end hook. Must never fail loudly: it runs as the harness exits.
      try {
        const hook = readStdinJson();
        const transcript = (flags.transcript as string) ?? hook.transcript_path;
        const label = `${hook.cwd ?? process.cwd()} · session ${hook.session_id ?? '?'}`;
        if (transcript) startHarvest(adapterFor(from ?? loadConfig().agent), transcript, label, self);
      } catch {
        /* a failed harvest only costs one session's candidates */
      }
      return;
    }
    case 'harvest-run':
      runHarvest(adapterFor(from ?? 'claude'), flags.input as string, flags.label as string);
      return;
    case 'notice': {
      const n = pendingCount();
      const adapter = adapterFor(from ?? 'claude');
      if (n > 0 && adapter.pendingNotice) console.log(adapter.pendingNotice(n));
      return;
    }
    case 'pending':
      console.log(`${pendingCount()} candidate rule(s) in ${paths.inbox}`);
      return;
    case 'prompt':
      console.log(reviewPrompt());
      return;
    case 'review': {
      const cfg = loadConfig();
      adapterFor(from ?? cfg.agent).launch(cfg.workspace, reviewPrompt());
      return;
    }
  }
  throw new Error('Usage: whet learn <review|pending|prompt>');
}

function task(sub: string, args: string[], flags: Record<string, string | boolean | undefined>): void {
  const cfg = loadConfig();
  const [id, ...rest] = args;
  switch (sub) {
    case 'new':
      if (!id || rest.length === 0) throw new Error('Usage: whet task new <id> <repo...>');
      print(newTask(cfg, id, rest, typeof flags.base === 'string' ? flags.base : undefined));
      console.log(`next: fill in the plan, then \`whet task run ${id}\``);
      return;
    case 'list': {
      const active = listTasks().filter((t) => t.status === 'active');
      if (active.length === 0) return console.log('No active tasks.');
      for (const t of active) {
        console.log(`${t.id}  (plan: ${planPath(cfg, t.id)})`);
        for (const s of taskStatus(cfg, t)) {
          const state = !s.exists ? 'missing' : `${s.ahead} commit(s)${s.dirty ? ', uncommitted changes' : ''}`;
          console.log(`  ${s.repo.padEnd(24)} ${state}`);
        }
      }
      return;
    }
  }
  if (!id) throw new Error(`Usage: whet task ${sub} <id>`);
  const t = loadTask(id);
  switch (sub) {
    case 'run':
      adapterFor(typeof flags.agent === 'string' ? flags.agent : cfg.agent).launch(cfg.workspace, runPrompt(cfg, t));
      return;
    case 'ship':
      print(shipTask(cfg, t, { to: flags.to as string | undefined, push: !flags['no-push'] }));
      return;
    case 'close':
      print(closeTask(cfg, t, Boolean(flags.force)));
      console.log('tip: `whet learn review` turns this task\'s corrections into rules');
      return;
  }
  throw new Error(USAGE);
}

function main(): void {
  const { positionals, values } = parseArgs({
    allowPositionals: true,
    options: {
      workspace: { type: 'string' },
      base: { type: 'string' },
      staging: { type: 'string' },
      agent: { type: 'string' },
      from: { type: 'string' },
      transcript: { type: 'string' },
      input: { type: 'string' },
      label: { type: 'string' },
      to: { type: 'string' },
      'no-push': { type: 'boolean' },
      force: { type: 'boolean' },
      help: { type: 'boolean', short: 'h' },
    },
  });
  const [cmd, sub, ...args] = positionals;
  if (!cmd || values.help) return console.log(USAGE);

  switch (cmd) {
    case 'init':
      return init(values);
    case 'install':
      if (!sub) throw new Error('Usage: whet install <claude|codex>');
      loadConfig();
      return print(adapterFor(sub).install());
    case 'config':
      return config(sub ? [sub, ...args] : []);
    case 'rules': {
      const rules = readRules();
      const lines = rules.split('\n').filter((l) => l.trim()).length;
      console.log(rules || '(no rules yet)');
      console.log(`\n${paths.rules}: ${lines}/${RULES_LINE_CAP} lines`);
      return;
    }
    case 'learn':
      return learn(sub ?? '', values);
    case 'task':
      return task(sub ?? '', args, values);
  }
  console.log(USAGE);
}

try {
  main();
} catch (e) {
  console.error((e as Error).message);
  process.exit(1);
}
