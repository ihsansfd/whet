use std::path::Path;

use anyhow::Result;
use whet_core::agent::Agent;
use whet_core::config::{AgentName, Paths, home_dir};

use crate::link::link_rules;
use crate::run::{capture, interactive};

/// Codex reads rules but has no session-end hook here, so harvesting is manual: `whet learn review`.
pub struct Codex;

impl Agent for Codex {
    fn name(&self) -> AgentName {
        AgentName::Codex
    }

    fn install(&self, paths: &Paths) -> Result<Vec<String>> {
        let mut log = link_rules(&home_dir().join(".codex").join("AGENTS.md"), paths)?;
        log.push(
            "no automatic harvest for codex yet: run `whet learn review` at the end of a session"
                .into(),
        );
        Ok(log)
    }

    fn complete(&self, prompt: &str, input: &str) -> Result<String> {
        capture(&["codex", "exec", &format!("{prompt}\n\n{input}")], "")
    }

    fn launch(&self, cwd: &Path, prompt: &str) -> Result<()> {
        interactive(&["codex", prompt], cwd)
    }
}
