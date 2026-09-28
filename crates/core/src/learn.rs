//! Turning corrections into rules: harvest → inbox → review.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;

use crate::agent::{Agent, Role, Turn};
use crate::config::Paths;
use crate::rules::{RULES_LINE_CAP, read_rules};

const MIN_USER_TURNS: usize = 3;
const MAX_INPUT_CHARS: usize = 60_000;

/// Renders turns for the harvester, newest last, trimmed from the front to fit.
pub fn render_conversation(turns: &[Turn]) -> String {
    let text = turns
        .iter()
        .map(|t| match t.role {
            Role::User => format!("USER: {}", first_chars(&t.text, 2000)),
            Role::Ai => format!("AI: {}", last_chars(&t.text, 400)),
        })
        .collect::<Vec<_>>()
        .join("\n");
    last_chars(&text, MAX_INPUT_CHARS).to_string()
}

fn first_chars(s: &str, n: usize) -> &str {
    s.char_indices().nth(n).map_or(s, |(i, _)| &s[..i])
}

fn last_chars(s: &str, n: usize) -> &str {
    let len = s.chars().count();
    if len <= n {
        return s;
    }
    s.char_indices().nth(len - n).map_or(s, |(i, _)| &s[i..])
}

pub fn harvest_prompt(rules: &str) -> String {
    format!(
        "Below is a conversation between a developer (USER) and an AI coding assistant (AI).
Find places where the USER corrected the AI: rejected an approach, fixed a convention, pushed back on a design, or repeated an instruction the AI ignored.
For each correction that would apply to future work (not a one-off detail of this task), write one candidate rule as a markdown bullet:
- <imperative rule, one sentence> — because <what went wrong, quoting the user briefly>
Skip anything these existing rules already cover:
{rules}

If there are no such corrections, output exactly: NONE
Output only the bullets or NONE."
    )
}

/// Called from a session-end hook. Returns immediately: the LLM call runs in a
/// detached child (`whet learn harvest-run`) so the harness can exit without waiting.
pub fn start_harvest(
    agent: &dyn Agent,
    transcript: &Path,
    label: &str,
    whet_exe: &Path,
) -> Result<String> {
    if std::env::var_os("WHET_HARVESTING").is_some() {
        return Ok("skipped: already inside a harvest".into());
    }
    if !transcript.exists() {
        return Ok(format!(
            "skipped: no transcript at {}",
            transcript.display()
        ));
    }
    let Some(turns) = agent.read_transcript(transcript)? else {
        return Ok(format!(
            "skipped: {} transcripts are not readable yet",
            agent.name()
        ));
    };
    if turns.iter().filter(|t| t.role == Role::User).count() < MIN_USER_TURNS {
        return Ok("skipped: session too short".into());
    }

    let nanos = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
    let dir = std::env::temp_dir().join(format!("whet-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir)?;
    let input = dir.join("conversation.txt");
    fs::write(&input, render_conversation(&turns))?;

    let mut cmd = Command::new(whet_exe);
    cmd.args([
        "learn",
        "harvest-run",
        "--from",
        agent.name().as_str(),
        "--label",
        label,
        "--input",
    ])
    .arg(&input)
    .env("WHET_HARVESTING", "1")
    .stdin(Stdio::null())
    .stdout(Stdio::null())
    .stderr(Stdio::null());
    detach(&mut cmd);
    cmd.spawn()?;
    Ok("harvest started".into())
}

/// Puts the child in its own process group so it outlives the harness that ran the hook.
#[cfg(unix)]
fn detach(cmd: &mut Command) {
    use std::os::unix::process::CommandExt;
    cmd.process_group(0);
}

#[cfg(windows)]
fn detach(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const DETACHED_PROCESS: u32 = 0x0000_0008;
    const CREATE_NEW_PROCESS_GROUP: u32 = 0x0000_0200;
    cmd.creation_flags(DETACHED_PROCESS | CREATE_NEW_PROCESS_GROUP);
}

/// Asks the agent for candidate rules and appends them to the inbox. Returns how many it found.
pub fn run_harvest(agent: &dyn Agent, paths: &Paths, input: &Path, label: &str) -> Result<usize> {
    let rules = read_rules(paths)?;
    let conversation = fs::read_to_string(input)?;
    if let Some(dir) = input.parent() {
        let _ = fs::remove_dir_all(dir);
    }
    let out = agent.complete(&harvest_prompt(&rules), &conversation)?;
    let out = out.trim();
    let bullets: Vec<&str> = out.lines().filter(|l| l.starts_with("- ")).collect();
    if out == "NONE" || bullets.is_empty() {
        return Ok(0);
    }
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M");
    fs::create_dir_all(&paths.home)?;
    let mut inbox = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&paths.inbox)?;
    write!(inbox, "\n## {stamp} · {label}\n{}\n", bullets.join("\n"))?;
    Ok(bullets.len())
}

pub fn pending_count(paths: &Paths) -> Result<usize> {
    if !paths.inbox.exists() {
        return Ok(0);
    }
    Ok(fs::read_to_string(&paths.inbox)?
        .lines()
        .filter(|l| l.starts_with("- "))
        .count())
}

/// Instructions for an agent session that turns corrections into rules with the user.
pub fn review_prompt(paths: &Paths) -> String {
    let rules = paths.rules.display();
    let inbox = paths.inbox.display();
    format!(
        "Help me turn corrections into rules. Whet keeps my global rules in {rules}; every AI tool I use reads that file. Every rule in it came from a correction I made, and the file stays at {RULES_LINE_CAP} lines or fewer.

1. Gather candidates:
   - This session, if we've been working together: every place I rejected an approach, fixed a convention, pushed back on a design, or repeated an instruction you ignored.
   - {inbox}: candidates harvested from earlier sessions (may not exist).
   Done when every correction and every inbox bullet is on your list.

2. Triage each candidate into exactly one bucket:
   - new: a rule for future work the rules file doesn't cover yet.
   - sharpen: an existing rule was broken or too vague; propose a rewrite of it.
   - promote: an existing rule was broken again; propose a lint rule, test or hook instead of more prose, and name the repo it belongs in.
   - domain: a business rule for one team or codebase; it goes in that repo's docs.
   - drop: a one-off detail of a single task, or already covered.

3. Show the triage as one table: candidate, bucket, proposed wording, target file. Write each rule as a positive imperative with a short reason (\"Do X, because Y\"). If the rules file would pass {RULES_LINE_CAP} lines, also propose which rules to merge or cut.

4. Apply only what I approve. Then remove the processed sections from {inbox}, and delete it once it's empty.
   Done when approved edits are in place and every inbox bullet is applied, dropped, or left for later at my request."
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn turn(role: Role, text: &str) -> Turn {
        Turn {
            role,
            text: text.into(),
        }
    }

    #[test]
    fn render_keeps_user_starts_and_ai_ends() {
        let long_ai = format!("{}END", "x".repeat(1000));
        let out =
            render_conversation(&[turn(Role::User, "fix the enum"), turn(Role::Ai, &long_ai)]);
        assert!(out.starts_with("USER: fix the enum\nAI: "));
        assert!(out.ends_with("END"));
        assert_eq!(
            out.lines().nth(1).unwrap().chars().count(),
            "AI: ".len() + 400
        );
    }

    #[test]
    fn render_trims_from_the_front_on_char_boundaries() {
        let turns: Vec<Turn> = (0..100)
            .map(|_| turn(Role::User, &"é".repeat(2000)))
            .collect();
        assert_eq!(render_conversation(&turns).chars().count(), MAX_INPUT_CHARS);
    }
}
