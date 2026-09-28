//! whet's domain: rules, learning and multi-repo tasks.
//!
//! Nothing here knows about a particular AI tool. Core reaches them only
//! through the [`agent::Agent`] port, which `whet-agents` implements. The CLI
//! (and later a desktop UI) is a thin layer on top.

pub mod agent;
pub mod config;
pub mod git;
pub mod learn;
pub mod rules;
pub mod tasks;
