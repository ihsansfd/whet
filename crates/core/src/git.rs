use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};

/// Runs git in `cwd` and returns its trimmed stdout, or fails with git's stderr.
pub fn git(cwd: &Path, args: &[&str]) -> Result<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .output()
        .context("could not run git. Is it installed?")?;
    if !out.status.success() {
        bail!(
            "git {} failed in {}:\n{}",
            args.join(" "),
            cwd.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn try_git(cwd: &Path, args: &[&str]) -> Option<String> {
    git(cwd, args).ok()
}

pub struct Worktree {
    pub path: PathBuf,
    pub branch: Option<String>,
}

pub fn worktrees(repo_dir: &Path) -> Result<Vec<Worktree>> {
    let list = git(repo_dir, &["worktree", "list", "--porcelain"])?;
    let mut out = Vec::new();
    for block in list.split("\n\n") {
        let mut path = None;
        let mut branch = None;
        for line in block.lines() {
            if let Some(p) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(p));
            }
            if let Some(b) = line.strip_prefix("branch refs/heads/") {
                branch = Some(b.to_string());
            }
        }
        if let Some(path) = path {
            out.push(Worktree { path, branch });
        }
    }
    Ok(out)
}

pub fn is_dirty(dir: &Path) -> Result<bool> {
    Ok(!git(dir, &["status", "--porcelain"])?.is_empty())
}

pub fn has_remote(repo_dir: &Path) -> bool {
    try_git(repo_dir, &["remote"]).is_some_and(|r| r.lines().any(|l| l == "origin"))
}

pub fn ref_exists(repo_dir: &Path, reference: &str) -> bool {
    try_git(repo_dir, &["rev-parse", "--verify", "--quiet", reference]).is_some()
}
