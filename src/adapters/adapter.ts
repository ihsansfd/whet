/** One AI harness (Claude Code, Codex, ...). Adapters are thin: the logic lives in core. */
export interface Adapter {
  name: string;
  /** Wire whet into the harness: rules link, hooks, commands. Returns one line per change made. */
  install(): string[];
  /** The conversation as plain turns, or undefined if this harness has no readable transcripts. */
  readTranscript?(file: string): Turn[];
  /** One-shot, non-interactive completion with no tools. */
  complete(prompt: string, input: string): string;
  /** Start an interactive agent session. */
  launch(cwd: string, prompt: string): void;
  /** What a session-start hook prints when candidate rules are waiting. */
  pendingNotice?(count: number): string;
}

export interface Turn {
  role: 'user' | 'ai';
  text: string;
}
