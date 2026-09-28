# whet

Every correction you give your AI coding agent should make it better next time, not vanish when the session ends.

whet sharpens the AI agent you already use (Claude Code, Codex, and more to come). It keeps one set of rules that every tool reads, turns your corrections into new rules, and runs multi-repo tasks without the git bookkeeping. It isn't another agent. It's the layer that makes the agent you have remember what you taught it.

## The problem

If you build with AI agents every day, the same cycle probably keeps repeating:

1. You plan a feature and let the agent implement it across several repos.
2. It fills a field in a way that breaks your team's convention, casts between enums owned by different services, or builds out a design that is getting harder to maintain, without saying so.
3. You correct it. It says "You're right…" and fixes it.
4. The context runs out. You start a new session, and your corrections are gone.
5. Next sprint, you type the same corrections again.

The usual explanation is that the model isn't smart enough. Working through the causes points somewhere else:

- **Your standards are unwritten.** Conventions like "a child record copies its parent's `source_reference_id`" live in people's heads. The agent fills the gap from an inconsistent codebase.
- **Your taste is tacit.** You know good code when you see it, but you can't write a style guide up front. Rules only show up as corrections, after something goes wrong.
- **Corrections live only in chat.** When the session ends, they're lost.
- **Two loops keep this going.** Refactor sessions use up the time you'd spend writing rules down. And frustration makes you give firm orders, which makes the model comply without thinking, including when your order is wrong.

A few things you can't change: models are stateless, context windows are finite, codebases are inconsistent, and models tend to agree with you. None of these does damage on its own. Each one causes harm only in combination with something you can change, and whet targets those.

Parallel work has its own friction. Running two tasks at once means creating worktrees in every repo, matching branch names, writing plans, managing terminals, and remembering which worktree holds the staging branch. That overhead is high enough that most people stay sequential.

## What whet does

### 1. One rules file for every tool

`~/.whet/rules.md` is the single source of truth. `whet install claude` links `~/.claude/CLAUDE.md` to it, and `whet install codex` links `~/.codex/AGENTS.md`. Edit it once and every tool follows. When you switch tools, the rules come with you.

The file stays at 40 lines or fewer. Long rules files get ignored, so the cap is deliberate: when a rule keeps being broken, whet suggests turning it into a lint rule or a test instead of adding more prose.

### 2. Corrections become rules

```
session ends ──► whet learn harvest ──► ~/.whet/inbox.md ──► whet learn review (/wrap-up) ──► rules.md
               (hook, runs in background)  (candidate rules)     (you approve each one)
```

- **Harvest.** When a session ends, a hook reads the transcript in the background, finds the places you corrected the agent, and writes candidate rules to an inbox. Harvesting never edits your rules.
- **Notice.** The next session tells you when candidates are waiting.
- **Review.** `whet learn review` (or `/wrap-up` in Claude Code) sorts each candidate into one of five buckets:

  | Bucket | Meaning |
  |---|---|
  | new | A rule the file doesn't cover yet |
  | sharpen | An existing rule was too vague |
  | promote | An existing rule was broken again; make it a lint rule or test |
  | domain | A team or codebase rule; it belongs in that repo's docs |
  | drop | A one-off, or already covered |

  Only what you approve gets written.

You approve every rule, because the agent can't judge your taste. Asking it to "learn from its mistakes" by itself tends to write down whatever it assumed.

### 3. Tasks across repos, without the git bookkeeping

A **task** (e.g. `INS-124`) is the unit you work with. whet handles the worktrees, branches, plans and staging for you.

```bash
whet task new INS-124 core-service deposit-engine   # a worktree per repo on branch INS-124, plus plan.md
whet task run INS-124                               # your agent, scoped to those worktrees, reviewing repo by repo
whet task ship INS-124                              # cherry-pick new commits onto the staging branch and push
whet task close INS-124                             # remove the worktrees (branches are kept)
whet task list                                      # every active task and its state per repo
```

- **Staging stays in one place.** Each repo gets a dedicated `<repo>-worktree/_staging` worktree that always holds the staging branch, so you never have to remember which worktree has it. If a task worktree is holding it, `ship` refuses and prints the command to free it.
- **Shipping twice is safe.** `ship` only picks commits staging doesn't already have (compared by patch, so earlier cherry-picks count).
- **Conflicts stop cleanly.** On a conflict, whet stops, names the repo and commit, and tells you how to resume. Repos that were already shipped stay shipped.
- **Review happens repo by repo.** `task run` tells the agent to stop after each repo so you can review before it continues, instead of reviewing a whole feature at the end.

## Install

whet is written in Rust and ships as a single binary. It also needs git.

```bash
cd whet
cargo build --release                             # writes target/release/whet
cp target/release/whet ~/.local/bin/whet
```

Then set it up:

```bash
whet init --workspace ~/code --staging testing/sprint-a
whet install claude                               # and/or: whet install codex
```

`whet install claude` does three things:
- Links `~/.claude/CLAUDE.md` to your rules. An existing file is imported if whet has no rules yet, and backed up either way.
- Adds a SessionEnd hook (harvest) and a SessionStart hook (notice) to `~/.claude/settings.json`, leaving your other hooks alone.
- Adds the `/wrap-up` command.

Running it again is safe.

## Commands

| Command | What it does |
|---|---|
| `whet init --workspace <dir>` | Set up `~/.whet`. Options: `--base`, `--staging`, `--agent`. |
| `whet install <claude\|codex>` | Wire whet into that tool. |
| `whet config [get <key> \| set <key> <value>]` | Show or change settings. |
| `whet rules` | Print the rules and their line count. |
| `whet learn review` | Start a session that turns corrections into rules. |
| `whet learn pending` | Count waiting candidates. |
| `whet task new <id> <repo...>` | Create the worktrees and plan. Adopts worktrees that already exist. |
| `whet task list` | Active tasks with commits ahead and uncommitted changes per repo. |
| `whet task run <id>` | Start your agent on the task. |
| `whet task ship <id> [--to <branch>] [--no-push]` | Cherry-pick onto staging. |
| `whet task close <id> [--force]` | Remove the worktrees. Refuses if there is uncommitted work, unless `--force`. |

## Configuration

`~/.whet/config.json`:

| Key | Default | Meaning |
|---|---|---|
| `workspace` | none | Folder holding the main checkout of each repo |
| `worktreeDir` | `{repo}-worktree/{task}` | Where task worktrees go, relative to the workspace |
| `plansDir` | `my-plans` | Where plans go: `<plansDir>/<task>/plan.md` |
| `baseBranch` | `master` | Branch that new tasks start from |
| `stagingBranch` | none | Branch that deploys to staging. Update it each sprint. |
| `agent` | `claude` | Agent used by `task run` and `learn review` |

Set `WHET_HOME` to keep whet's data somewhere other than `~/.whet`.

## How it's built

```
crates/
  core/        rules, learning, tasks, git: no knowledge of any AI tool (the Agent trait is its only port)
  agents/      one adapter per AI tool: install, read transcripts, one-shot completion, launch a session
  cli/         the `whet` binary
```

The core owns all the logic. An adapter is the thin layer that knows where a particular tool keeps its rules file, how its hooks fire, and how to call it. Supporting a new tool means writing one adapter. A desktop UI would call the same core.

## Status and roadmap

whet is at version 0.1: early, and shaped by one developer's workflow so far.

Works today:
- Rules linking for Claude Code and Codex
- Automatic harvesting for Claude Code
- Tasks: new, list, run, ship, close

Next:
- Open merge requests per repo (GitLab and GitHub)
- Automatic harvesting for Codex and other tools
- Company and personal project profiles with different rule sets
- A desktop UI (macOS, Windows, Linux, later web): a task board plus the rules inbox, on the same core

## Development

```bash
cargo test                  # unit tests, plus end-to-end tests against throwaway git repos with a fake remote
cargo clippy --all-targets
cargo build --release       # single binary at target/release/whet
```

Hooks installed by `whet install` call whet by the binary's absolute path, so they work even when your PATH isn't loaded.
