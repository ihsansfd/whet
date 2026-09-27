import os from 'node:os';
import path from 'node:path';
import type { Adapter } from './adapter.ts';
import { linkRules } from './link.ts';
import { capture, interactive } from './run.ts';

/** Codex reads rules but has no session-end hook here, so harvesting is manual: `whet learn review`. */
export function codexAdapter(): Adapter {
  return {
    name: 'codex',

    install() {
      return [
        ...linkRules(path.join(os.homedir(), '.codex', 'AGENTS.md')),
        'no automatic harvest for codex yet: run `whet learn review` at the end of a session',
      ];
    },

    complete(prompt, input) {
      return capture(['codex', 'exec', `${prompt}\n\n${input}`], '');
    },

    launch(cwd, prompt) {
      interactive(['codex', prompt], cwd);
    },
  };
}
