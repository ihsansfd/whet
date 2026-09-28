use std::path::Path;

use anyhow::Result;

use crate::config::{AgentName, Paths};

/// One AI harness (Claude Code, Codex, ...). Adapters are thin: the logic lives in core.
pub trait Agent {
    fn name(&self) -> AgentName;

    /// Wires whet into the harness: rules link, hooks, commands. Returns one line per change made.
    fn install(&self, paths: &Paths) -> Result<Vec<String>>;

    /// The conversation as plain turns, or `None` if this harness has no readable transcripts.
    fn read_transcript(&self, _file: &Path) -> Result<Option<Vec<Turn>>> {
        Ok(None)
    }

    /// One-shot, non-interactive completion with no tools.
    fn complete(&self, prompt: &str, input: &str) -> Result<String>;

    /// Starts an interactive agent session attached to this terminal.
    fn launch(&self, cwd: &Path, prompt: &str) -> Result<()>;

    /// What a session-start hook prints when candidate rules are waiting.
    fn pending_notice(&self, _count: usize) -> Option<String> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Ai,
}

#[derive(Debug, Clone)]
pub struct Turn {
    pub role: Role,
    pub text: String,
}
