use std::fs;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

use anyhow::{Context, Result};
use regex::Regex;
use serde_json::{Value, json};
use whet_core::agent::{Agent, Role, Turn};
use whet_core::config::{AgentName, Paths, home_dir};

use crate::link::link_rules;
use crate::run::{capture, interactive};

/// Hooks whet owns, from every version: the Rust binary, the Bun script, the
/// Node build and the pre-whet shell scripts. Replacing them keeps install idempotent.
static WHET_HOOK: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r#"whet(\.js)?["']? learn|cli\.(js|ts)["']? learn|harvest-corrections\.sh|rules-inbox-notice\.sh"#)
        .unwrap()
});

pub struct Claude {
    whet_command: String,
    home: PathBuf,
}

impl Claude {
    pub fn new(whet_command: &str) -> Self {
        Self {
            whet_command: whet_command.into(),
            home: home_dir().join(".claude"),
        }
    }

    fn install_hooks(&self) -> Result<String> {
        let file = self.home.join("settings.json");
        let mut settings: Value = if file.exists() {
            serde_json::from_str(&fs::read_to_string(&file)?)
                .with_context(|| format!("{} is not valid JSON", file.display()))?
        } else {
            json!({})
        };
        let root = settings
            .as_object_mut()
            .with_context(|| format!("{} is not a JSON object", file.display()))?;
        let hooks = root.entry("hooks").or_insert_with(|| json!({}));
        let hooks = hooks
            .as_object_mut()
            .context("settings.hooks is not a JSON object")?;
        for (event, sub) in [("SessionEnd", "harvest"), ("SessionStart", "notice")] {
            let mut kept: Vec<Value> = match hooks.get(event) {
                Some(Value::Array(entries)) => entries
                    .iter()
                    .filter(|e| !is_whet_hook(e))
                    .cloned()
                    .collect(),
                _ => Vec::new(),
            };
            kept.push(json!({
                "hooks": [{ "type": "command", "command": format!("{} learn {sub} --from claude", self.whet_command), "timeout": 10 }]
            }));
            hooks.insert(event.into(), Value::Array(kept));
        }
        fs::write(&file, serde_json::to_string_pretty(&settings)? + "\n")?;
        Ok(format!(
            "hooks: SessionEnd → learn harvest, SessionStart → learn notice ({})",
            file.display()
        ))
    }

    fn install_wrap_up(&self) -> Result<String> {
        let skill = self.home.join("skills").join("wrap-up").join("SKILL.md");
        if let Some(dir) = skill.parent() {
            fs::create_dir_all(dir)?;
        }
        fs::write(
            &skill,
            format!(
                "---
name: wrap-up
description: Turn corrections from this session and whet's inbox into rules, with my approval.
disable-model-invocation: true
---

Run `{} learn prompt` and follow the instructions it prints.
",
                self.whet_command
            ),
        )?;
        Ok(format!("command: /wrap-up ({})", skill.display()))
    }
}

fn is_whet_hook(entry: &Value) -> bool {
    entry["hooks"].as_array().is_some_and(|hooks| {
        hooks
            .iter()
            .any(|h| h["command"].as_str().is_some_and(|c| WHET_HOOK.is_match(c)))
    })
}

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

impl Agent for Claude {
    fn name(&self) -> AgentName {
        AgentName::Claude
    }

    fn install(&self, paths: &Paths) -> Result<Vec<String>> {
        let mut log = link_rules(&self.home.join("CLAUDE.md"), paths)?;
        log.push(self.install_hooks()?);
        log.push(self.install_wrap_up()?);
        Ok(log)
    }

    fn read_transcript(&self, file: &Path) -> Result<Option<Vec<Turn>>> {
        let mut turns = Vec::new();
        for line in fs::read_to_string(file)?.lines() {
            let Ok(entry) = serde_json::from_str::<Value>(line) else {
                continue;
            };
            let text = text_of(&entry["message"]["content"]);
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            match entry["type"].as_str() {
                Some("assistant") => turns.push(Turn {
                    role: Role::Ai,
                    text: text.into(),
                }),
                // Skip injected blocks (<system-reminder>, command output) and loaded skill bodies.
                Some("user")
                    if !text.starts_with('<')
                        && !text.starts_with("Base directory for this skill") =>
                {
                    turns.push(Turn {
                        role: Role::User,
                        text: text.into(),
                    })
                }
                _ => {}
            }
        }
        Ok(Some(turns))
    }

    fn complete(&self, prompt: &str, input: &str) -> Result<String> {
        capture(
            &[
                "claude",
                "-p",
                "--model",
                "sonnet",
                "--tools",
                "",
                "--no-session-persistence",
                "--setting-sources",
                "",
                prompt,
            ],
            input,
        )
    }

    fn launch(&self, cwd: &Path, prompt: &str) -> Result<()> {
        interactive(&["claude", prompt], cwd)
    }

    fn pending_notice(&self, count: usize) -> Option<String> {
        Some(
            json!({
                "systemMessage": format!("{count} candidate rule(s) waiting in whet's inbox. Run /wrap-up to review."),
                "hookSpecificOutput": {
                    "hookEventName": "SessionStart",
                    "additionalContext": format!("{count} candidate rules from earlier sessions are waiting in whet's inbox. Mention this once, briefly, at a natural pause, and suggest /wrap-up. Don't apply them yourself."),
                },
            })
            .to_string(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognizes_every_generation_of_whet_hook() {
        for command in [
            r#""/Users/me/.local/bin/whet" learn harvest --from claude"#,
            r#""/Users/me/.bun/bin/bun" "/Users/me/whet/src/cli.ts" learn harvest --from claude"#,
            "node /opt/whet/dist/cli.js learn notice",
            "~/.claude/hooks/harvest-corrections.sh",
        ] {
            assert!(
                is_whet_hook(&json!({ "hooks": [{ "command": command }] })),
                "{command}"
            );
        }
        assert!(!is_whet_hook(
            &json!({ "hooks": [{ "command": "prettier --write" }] })
        ));
    }
}
