import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Adapter, Turn } from './adapter.ts';
import { linkRules } from './link.ts';
import { capture, interactive } from './run.ts';

const claudeHome = path.join(os.homedir(), '.claude');

type HookEntry = { matcher?: string; hooks: { type: string; command: string; timeout?: number }[] };

/** Removes hooks whet owns (current and pre-whet versions) so install stays idempotent. */
function isWhetHook(entry: HookEntry): boolean {
  return entry.hooks.some((h) => /whet(\.js)?["']? learn|cli\.js["']? learn|harvest-corrections\.sh|rules-inbox-notice\.sh/.test(h.command));
}

function textOf(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .filter((b) => b?.type === 'text')
    .map((b) => b.text as string)
    .join('\n');
}

export function claudeAdapter(cli: string): Adapter {
  return {
    name: 'claude',

    install() {
      const log = linkRules(path.join(claudeHome, 'CLAUDE.md'));

      const settingsFile = path.join(claudeHome, 'settings.json');
      const settings = fs.existsSync(settingsFile) ? JSON.parse(fs.readFileSync(settingsFile, 'utf8')) : {};
      settings.hooks ??= {};
      for (const [event, sub] of [
        ['SessionEnd', 'harvest'],
        ['SessionStart', 'notice'],
      ] as const) {
        const kept = ((settings.hooks[event] ?? []) as HookEntry[]).filter((e) => !isWhetHook(e));
        kept.push({ hooks: [{ type: 'command', command: `${cli} learn ${sub} --from claude`, timeout: 10 }] });
        settings.hooks[event] = kept;
      }
      fs.writeFileSync(settingsFile, JSON.stringify(settings, null, 2) + '\n');
      log.push(`hooks: SessionEnd → learn harvest, SessionStart → learn notice (${settingsFile})`);

      const skill = path.join(claudeHome, 'skills', 'wrap-up', 'SKILL.md');
      fs.mkdirSync(path.dirname(skill), { recursive: true });
      fs.writeFileSync(
        skill,
        `---
name: wrap-up
description: Turn corrections from this session and whet's inbox into rules, with my approval.
disable-model-invocation: true
---

Run \`${cli} learn prompt\` and follow the instructions it prints.
`,
      );
      log.push(`command: /wrap-up (${skill})`);
      return log;
    },

    readTranscript(file) {
      const turns: Turn[] = [];
      for (const line of fs.readFileSync(file, 'utf8').split('\n')) {
        if (!line.trim()) continue;
        let entry: { type?: string; message?: { content?: unknown } };
        try {
          entry = JSON.parse(line);
        } catch {
          continue;
        }
        const text = textOf(entry.message?.content).trim();
        if (!text) continue;
        if (entry.type === 'assistant') turns.push({ role: 'ai', text });
        // Skip injected blocks (<system-reminder>, command output) and loaded skill bodies.
        else if (entry.type === 'user' && !text.startsWith('<') && !text.startsWith('Base directory for this skill')) {
          turns.push({ role: 'user', text });
        }
      }
      return turns;
    },

    complete(prompt, input) {
      return capture(
        ['claude', '-p', '--model', 'sonnet', '--tools', '', '--no-session-persistence', '--setting-sources', '', prompt],
        input,
      );
    },

    launch(cwd, prompt) {
      interactive(['claude', prompt], cwd);
    },

    pendingNotice(count) {
      return JSON.stringify({
        systemMessage: `${count} candidate rule(s) waiting in whet's inbox. Run /wrap-up to review.`,
        hookSpecificOutput: {
          hookEventName: 'SessionStart',
          additionalContext: `${count} candidate rules from earlier sessions are waiting in whet's inbox. Mention this once, briefly, at a natural pause, and suggest /wrap-up. Don't apply them yourself.`,
        },
      });
    },
  };
}
