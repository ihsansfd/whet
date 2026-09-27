import { spawn } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import type { Adapter, Turn } from '../adapters/adapter.ts';
import { paths } from './config.ts';

export const RULES_LINE_CAP = 40;
const MIN_USER_TURNS = 3;
const MAX_INPUT_CHARS = 60_000;

/** Renders turns for the harvester, newest last, trimmed from the front to fit. */
export function renderConversation(turns: Turn[]): string {
  const text = turns
    .map((t) => (t.role === 'user' ? `USER: ${t.text.slice(0, 2000)}` : `AI: ${t.text.slice(-400)}`))
    .join('\n');
  return text.slice(-MAX_INPUT_CHARS);
}

export function harvestPrompt(rules: string): string {
  return `Below is a conversation between a developer (USER) and an AI coding assistant (AI).
Find places where the USER corrected the AI: rejected an approach, fixed a convention, pushed back on a design, or repeated an instruction the AI ignored.
For each correction that would apply to future work (not a one-off detail of this task), write one candidate rule as a markdown bullet:
- <imperative rule, one sentence> — because <what went wrong, quoting the user briefly>
Skip anything these existing rules already cover:
${rules}

If there are no such corrections, output exactly: NONE
Output only the bullets or NONE.`;
}

/**
 * Called from a session-end hook. Returns immediately: the LLM call runs in a
 * detached child so the harness can exit without waiting.
 */
export function startHarvest(adapter: Adapter, transcript: string, label: string, self: string[]): string {
  if (process.env.WHET_HARVESTING) return 'skipped: already inside a harvest';
  if (!adapter.readTranscript) return `skipped: ${adapter.name} transcripts are not readable yet`;
  if (!fs.existsSync(transcript)) return `skipped: no transcript at ${transcript}`;

  const turns = adapter.readTranscript(transcript);
  if (turns.filter((t) => t.role === 'user').length < MIN_USER_TURNS) return 'skipped: session too short';

  const input = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'whet-')), 'conversation.txt');
  fs.writeFileSync(input, renderConversation(turns));
  const [cmd, ...args] = self;
  const child = spawn(cmd!, [...args, 'learn', 'harvest-run', '--from', adapter.name, '--input', input, '--label', label], {
    detached: true,
    stdio: 'ignore',
    env: { ...process.env, WHET_HARVESTING: '1' },
  });
  child.unref();
  return 'harvest started';
}

export function runHarvest(adapter: Adapter, input: string, label: string): number {
  const rules = readRules();
  const out = adapter.complete(harvestPrompt(rules), fs.readFileSync(input, 'utf8')).trim();
  fs.rmSync(path.dirname(input), { recursive: true, force: true });
  const bullets = out.split('\n').filter((l) => l.startsWith('- '));
  if (out === 'NONE' || bullets.length === 0) return 0;
  const d = new Date();
  const pad = (n: number) => String(n).padStart(2, '0');
  const stamp = `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
  fs.appendFileSync(paths.inbox, `\n## ${stamp} · ${label}\n${bullets.join('\n')}\n`);
  return bullets.length;
}

export function pendingCount(): number {
  if (!fs.existsSync(paths.inbox)) return 0;
  return fs.readFileSync(paths.inbox, 'utf8').split('\n').filter((l) => l.startsWith('- ')).length;
}

export function readRules(): string {
  return fs.existsSync(paths.rules) ? fs.readFileSync(paths.rules, 'utf8') : '';
}

/** Instructions for an agent session that turns corrections into rules with the user. */
export function reviewPrompt(): string {
  return `Help me turn corrections into rules. Whet keeps my global rules in ${paths.rules}; every AI tool I use reads that file. Every rule in it came from a correction I made, and the file stays at ${RULES_LINE_CAP} lines or fewer.

1. Gather candidates:
   - This session, if we've been working together: every place I rejected an approach, fixed a convention, pushed back on a design, or repeated an instruction you ignored.
   - ${paths.inbox}: candidates harvested from earlier sessions (may not exist).
   Done when every correction and every inbox bullet is on your list.

2. Triage each candidate into exactly one bucket:
   - new: a rule for future work the rules file doesn't cover yet.
   - sharpen: an existing rule was broken or too vague; propose a rewrite of it.
   - promote: an existing rule was broken again; propose a lint rule, test or hook instead of more prose, and name the repo it belongs in.
   - domain: a business rule for one team or codebase; it goes in that repo's docs.
   - drop: a one-off detail of a single task, or already covered.

3. Show the triage as one table: candidate, bucket, proposed wording, target file. Write each rule as a positive imperative with a short reason ("Do X, because Y"). If the rules file would pass ${RULES_LINE_CAP} lines, also propose which rules to merge or cut.

4. Apply only what I approve. Then remove the processed sections from ${paths.inbox}, and delete it once it's empty.
   Done when approved edits are in place and every inbox bullet is applied, dropped, or left for later at my request.`;
}
