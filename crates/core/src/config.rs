use std::fmt;
use std::fs;
use std::path::PathBuf;
use std::str::FromStr;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

/// Where whet keeps its data: `~/.whet`, or `$WHET_HOME` when set.
#[derive(Debug, Clone)]
pub struct Paths {
    pub home: PathBuf,
    pub config: PathBuf,
    pub rules: PathBuf,
    pub inbox: PathBuf,
    pub tasks: PathBuf,
}

impl Paths {
    pub fn from_env() -> Self {
        let home = std::env::var_os("WHET_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".whet"));
        Self::at(home)
    }

    pub fn at(home: PathBuf) -> Self {
        Self {
            config: home.join("config.json"),
            rules: home.join("rules.md"),
            inbox: home.join("inbox.md"),
            tasks: home.join("tasks"),
            home,
        }
    }
}

#[allow(deprecated)] // std::env::home_dir is correct on every platform since Rust 1.85
pub fn home_dir() -> PathBuf {
    std::env::home_dir().expect("could not determine the home directory")
}

pub fn expand_home(p: &str) -> PathBuf {
    match p.strip_prefix('~') {
        Some("") => home_dir(),
        Some(rest) if rest.starts_with('/') => home_dir().join(&rest[1..]),
        _ => PathBuf::from(p),
    }
}

/// The AI harnesses whet supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AgentName {
    #[default]
    Claude,
    Codex,
}

impl AgentName {
    pub fn as_str(self) -> &'static str {
        match self {
            AgentName::Claude => "claude",
            AgentName::Codex => "codex",
        }
    }
}

impl fmt::Display for AgentName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for AgentName {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "claude" => Ok(AgentName::Claude),
            "codex" => Ok(AgentName::Codex),
            other => bail!("Unknown agent \"{other}\". Supported: claude, codex."),
        }
    }
}

pub const CONFIG_KEYS: [&str; 6] = [
    "workspace",
    "worktreeDir",
    "plansDir",
    "baseBranch",
    "stagingBranch",
    "agent",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Where task worktrees go, relative to workspace. {repo} and {task} are replaced.
    #[serde(default = "default_worktree_dir")]
    pub worktree_dir: String,
    /// Where task plans go, relative to workspace.
    #[serde(default = "default_plans_dir")]
    pub plans_dir: String,
    #[serde(default = "default_base_branch")]
    pub base_branch: String,
    #[serde(default)]
    pub agent: AgentName,
    /// Directory holding the main checkout of every repo.
    pub workspace: PathBuf,
    /// Branch that deploys to staging. Changes per sprint, so it's a setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub staging_branch: Option<String>,
}

fn default_worktree_dir() -> String {
    "{repo}-worktree/{task}".into()
}
fn default_plans_dir() -> String {
    "my-plans".into()
}
fn default_base_branch() -> String {
    "master".into()
}

impl Config {
    pub fn new(workspace: PathBuf) -> Self {
        Self {
            worktree_dir: default_worktree_dir(),
            plans_dir: default_plans_dir(),
            base_branch: default_base_branch(),
            agent: AgentName::default(),
            workspace,
            staging_branch: None,
        }
    }

    pub fn load(paths: &Paths) -> Result<Self> {
        if !paths.config.exists() {
            bail!("whet is not set up yet. Run `whet init --workspace <dir>` first.");
        }
        let raw = fs::read_to_string(&paths.config)?;
        let mut cfg: Config = serde_json::from_str(&raw)
            .with_context(|| format!("{} is not valid", paths.config.display()))?;
        cfg.workspace = expand_home(&cfg.workspace.to_string_lossy());
        Ok(cfg)
    }

    pub fn save(&self, paths: &Paths) -> Result<()> {
        fs::create_dir_all(&paths.home)?;
        fs::write(&paths.config, serde_json::to_string_pretty(self)? + "\n")?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<String> {
        Ok(match key {
            "workspace" => self.workspace.display().to_string(),
            "worktreeDir" => self.worktree_dir.clone(),
            "plansDir" => self.plans_dir.clone(),
            "baseBranch" => self.base_branch.clone(),
            "stagingBranch" => self.staging_branch.clone().unwrap_or_default(),
            "agent" => self.agent.to_string(),
            _ => bail!(unknown_key()),
        })
    }

    pub fn set(&mut self, key: &str, value: &str) -> Result<()> {
        match key {
            "workspace" => self.workspace = expand_home(value),
            "worktreeDir" => self.worktree_dir = value.into(),
            "plansDir" => self.plans_dir = value.into(),
            "baseBranch" => self.base_branch = value.into(),
            "stagingBranch" => self.staging_branch = Some(value.into()),
            "agent" => self.agent = value.parse()?,
            _ => bail!(unknown_key()),
        }
        Ok(())
    }
}

fn unknown_key() -> String {
    format!("Unknown key. Keys: {}", CONFIG_KEYS.join(", "))
}
