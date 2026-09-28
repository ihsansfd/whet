//! One adapter per AI harness. Each knows where its tool keeps the rules file,
//! how its hooks fire and how to call it; the logic stays in `whet-core`.

mod claude;
mod codex;
mod link;
mod run;

use whet_core::agent::Agent;
use whet_core::config::AgentName;

pub use claude::Claude;
pub use codex::Codex;

/// The adapter for `name`. `whet_command` is how hooks and generated commands
/// call whet: quoted absolute paths, so they work without PATH.
pub fn for_name(name: AgentName, whet_command: &str) -> Box<dyn Agent> {
    match name {
        AgentName::Claude => Box::new(Claude::new(whet_command)),
        AgentName::Codex => Box::new(Codex),
    }
}
