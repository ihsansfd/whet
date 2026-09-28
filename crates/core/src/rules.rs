use std::fs;

use anyhow::Result;

use crate::config::Paths;

/// The rules file stays this short so every tool keeps reading it.
pub const RULES_LINE_CAP: usize = 40;

pub fn read_rules(paths: &Paths) -> Result<String> {
    if !paths.rules.exists() {
        return Ok(String::new());
    }
    Ok(fs::read_to_string(&paths.rules)?)
}

/// Lines that count toward the cap: blank lines are free.
pub fn line_count(rules: &str) -> usize {
    rules.lines().filter(|l| !l.trim().is_empty()).count()
}
