//! Multi-repo tasks: a worktree per repo, a plan, and shipping onto staging.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

use crate::config::{Config, Paths};
use crate::git::{git, has_remote, is_dirty, ref_exists, try_git, worktrees};

/// The worktree that permanently holds the staging branch, so no task worktree ever does.
pub const STAGING_WORKTREE: &str = "_staging";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: String,
    pub repos: Vec<String>,
    pub base: String,
    pub created_at: String,
    pub status: TaskStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    Active,
    Closed,
}

#[derive(Debug, Clone)]
pub struct RepoStatus {
    pub repo: String,
    pub path: PathBuf,
    pub exists: bool,
    pub dirty: bool,
    pub ahead: u32,
}

/// Task records: one JSON file per task in `~/.whet/tasks`.
struct TaskStore {
    dir: PathBuf,
}

impl TaskStore {
    fn file(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn load(&self, id: &str) -> Result<Task> {
        let file = self.file(id);
        if !file.exists() {
            bail!("No task {id}. See `whet task list`.");
        }
        Ok(serde_json::from_str(&fs::read_to_string(file)?)?)
    }

    fn save(&self, task: &Task) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        fs::write(
            self.file(&task.id),
            serde_json::to_string_pretty(task)? + "\n",
        )?;
        Ok(())
    }

    fn list(&self) -> Result<Vec<Task>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }
        let mut tasks = Vec::new();
        for entry in fs::read_dir(&self.dir)? {
            let path = entry?.path();
            if path.extension().is_some_and(|e| e == "json") {
                tasks.push(serde_json::from_str::<Task>(&fs::read_to_string(&path)?)?);
            }
        }
        tasks.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(tasks)
    }
}

pub struct Tasks {
    cfg: Config,
    store: TaskStore,
}

impl Tasks {
    pub fn new(cfg: Config, paths: &Paths) -> Self {
        Self {
            cfg,
            store: TaskStore {
                dir: paths.tasks.clone(),
            },
        }
    }

    pub fn repo_dir(&self, repo: &str) -> PathBuf {
        self.cfg.workspace.join(repo)
    }

    pub fn worktree_path(&self, repo: &str, task: &str) -> PathBuf {
        self.cfg.workspace.join(
            self.cfg
                .worktree_dir
                .replace("{repo}", repo)
                .replace("{task}", task),
        )
    }

    pub fn plan_path(&self, id: &str) -> PathBuf {
        self.cfg
            .workspace
            .join(&self.cfg.plans_dir)
            .join(id)
            .join("plan.md")
    }

    pub fn load(&self, id: &str) -> Result<Task> {
        self.store.load(id)
    }

    pub fn active(&self) -> Result<Vec<Task>> {
        Ok(self
            .store
            .list()?
            .into_iter()
            .filter(|t| t.status == TaskStatus::Active)
            .collect())
    }

    /// Creates a worktree per repo on branch `id` and a plan. Adopts worktrees that already exist.
    pub fn create(&self, id: &str, repos: &[String], base: Option<&str>) -> Result<Vec<String>> {
        let base = base.unwrap_or(&self.cfg.base_branch);
        if self.store.file(id).exists() {
            bail!("Task {id} already exists.");
        }
        for repo in repos {
            if !self.repo_dir(repo).join(".git").exists() {
                bail!(
                    "{repo} is not a git repo in {}.",
                    self.cfg.workspace.display()
                );
            }
        }

        let mut log = Vec::new();
        for repo in repos {
            let dir = self.repo_dir(repo);
            let wt = self.worktree_path(repo, id);
            let wt_arg = wt.to_string_lossy();
            if wt.exists() {
                log.push(format!(
                    "{repo}: worktree already at {}, keeping it",
                    wt.display()
                ));
                continue;
            }
            if ref_exists(&dir, &format!("refs/heads/{id}")) {
                git(&dir, &["worktree", "add", &wt_arg, id])?;
                log.push(format!("{repo}: {} on existing branch {id}", wt.display()));
            } else {
                let from = base_ref(&dir, base)?;
                git(
                    &dir,
                    &["worktree", "add", "--no-track", "-b", id, &wt_arg, &from],
                )?;
                log.push(format!(
                    "{repo}: {} on new branch {id} from {from}",
                    wt.display()
                ));
            }
        }

        let plan = self.plan_path(id);
        if !plan.exists() {
            if let Some(dir) = plan.parent() {
                fs::create_dir_all(dir)?;
            }
            let lines: Vec<String> = repos
                .iter()
                .map(|r| format!("- {r}: {}", self.worktree_path(r, id).display()))
                .collect();
            fs::write(
                &plan,
                format!(
                    "# {id}\n\n## Worktrees\n{}\n\n## Goal\n\n## Design\n",
                    lines.join("\n")
                ),
            )?;
            log.push(format!("plan: {}", plan.display()));
        }

        self.store.save(&Task {
            id: id.into(),
            repos: repos.to_vec(),
            base: base.into(),
            created_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
            status: TaskStatus::Active,
        })?;
        Ok(log)
    }

    pub fn status(&self, task: &Task) -> Result<Vec<RepoStatus>> {
        task.repos
            .iter()
            .map(|repo| {
                let path = self.worktree_path(repo, &task.id);
                if !path.exists() {
                    return Ok(RepoStatus {
                        repo: repo.clone(),
                        path,
                        exists: false,
                        dirty: false,
                        ahead: 0,
                    });
                }
                let base = if has_remote(&path) {
                    format!("origin/{}", task.base)
                } else {
                    task.base.clone()
                };
                let ahead = try_git(&path, &["rev-list", "--count", &format!("{base}..HEAD")])
                    .and_then(|n| n.parse().ok())
                    .unwrap_or(0);
                Ok(RepoStatus {
                    repo: repo.clone(),
                    dirty: is_dirty(&path)?,
                    path,
                    exists: true,
                    ahead,
                })
            })
            .collect()
    }

    /// Cherry-picks the task's commits that staging doesn't have yet, repo by repo.
    pub fn ship(&self, task: &Task, to: Option<&str>, push: bool) -> Result<Vec<String>> {
        let Some(staging) = to.or(self.cfg.staging_branch.as_deref()) else {
            bail!(
                "No staging branch set. Run `whet config set stagingBranch <branch>` or pass --to."
            );
        };

        let mut log = Vec::new();
        for repo in &task.repos {
            let dir = self.repo_dir(repo);
            let staging_wt = self.ensure_staging_worktree(repo, staging)?;
            if is_dirty(&staging_wt)? {
                bail!(
                    "{repo}: {} has uncommitted changes (an unfinished cherry-pick?).",
                    staging_wt.display()
                );
            }
            let remote = has_remote(&dir);
            if remote {
                git(&staging_wt, &["pull", "--ff-only", "origin", staging])?;
            }

            let base = if remote {
                format!("origin/{}", task.base)
            } else {
                task.base.clone()
            };
            let fork_point = git(&dir, &["merge-base", &task.id, &base])?;
            let cherry = git(&dir, &["cherry", staging, &task.id, &fork_point])?;
            let picks: Vec<&str> = cherry
                .lines()
                .filter_map(|l| l.strip_prefix("+ "))
                .collect();

            if picks.is_empty() {
                log.push(format!("{repo}: nothing new for {staging}"));
                continue;
            }
            for sha in &picks {
                if try_git(&staging_wt, &["cherry-pick", "-x", sha]).is_none() {
                    bail!(
                        "{repo}: cherry-pick of {} conflicted in {}.\n\
                         Resolve it there (your agent's /resolving-merge-conflicts works), run `git cherry-pick --continue`, \
                         then run `whet task ship {}` again. Earlier repos are already done.",
                        &sha[..sha.len().min(8)],
                        staging_wt.display(),
                        task.id
                    );
                }
            }
            if push && remote {
                git(&staging_wt, &["push", "origin", staging])?;
            }
            let note = if push { "" } else { " (not pushed)" };
            log.push(format!(
                "{repo}: {} commit(s) onto {staging}{note}",
                picks.len()
            ));
        }
        Ok(log)
    }

    /// Makes sure `repo`'s staging worktree exists and holds the staging branch.
    fn ensure_staging_worktree(&self, repo: &str, staging: &str) -> Result<PathBuf> {
        let dir = self.repo_dir(repo);
        let staging_wt = self.worktree_path(repo, STAGING_WORKTREE);
        let wt_arg = staging_wt.to_string_lossy();
        let holder = worktrees(&dir)?
            .into_iter()
            .find(|w| w.branch.as_deref() == Some(staging));
        match holder {
            Some(h) if !same_path(&h.path, &staging_wt) => bail!(
                "{repo}: {staging} is checked out in {}, and whet keeps it in {}.\n\
                 Commit or stash anything there, then free it with:\n  git -C \"{}\" switch --detach",
                h.path.display(),
                staging_wt.display(),
                h.path.display()
            ),
            Some(_) => {}
            None => {
                if has_remote(&dir) {
                    try_git(&dir, &["fetch", "origin", staging]);
                }
                if ref_exists(&dir, &format!("refs/heads/{staging}")) {
                    git(&dir, &["worktree", "add", &wt_arg, staging])?;
                } else if ref_exists(&dir, &format!("refs/remotes/origin/{staging}")) {
                    git(
                        &dir,
                        &[
                            "worktree",
                            "add",
                            "--track",
                            "-b",
                            staging,
                            &wt_arg,
                            &format!("origin/{staging}"),
                        ],
                    )?;
                } else {
                    bail!("{repo}: no branch {staging} locally or on origin.");
                }
            }
        }
        Ok(staging_wt)
    }

    /// Removes the task's worktrees (branches are kept). Refuses on uncommitted work unless `force`.
    pub fn close(&self, task: &Task, force: bool) -> Result<Vec<String>> {
        let statuses = self.status(task)?;
        if !force && let Some(s) = statuses.iter().find(|s| s.exists && s.dirty) {
            bail!(
                "{}: {} has uncommitted changes. Commit them, or pass --force.",
                s.repo,
                s.path.display()
            );
        }
        let mut log = Vec::new();
        for s in statuses.iter().filter(|s| s.exists) {
            let path = s.path.to_string_lossy();
            let mut args = vec!["worktree", "remove"];
            if force {
                args.push("--force");
            }
            args.push(&path);
            git(&self.repo_dir(&s.repo), &args)?;
            log.push(format!(
                "{}: removed {} (branch {} kept)",
                s.repo,
                s.path.display(),
                task.id
            ));
        }
        self.store.save(&Task {
            status: TaskStatus::Closed,
            ..task.clone()
        })?;
        Ok(log)
    }

    pub fn run_prompt(&self, task: &Task) -> String {
        let wts: Vec<String> = task
            .repos
            .iter()
            .map(|r| format!("- {r}: {}", self.worktree_path(r, &task.id).display()))
            .collect();
        format!(
            "We're working on task {}. The plan is in {}; read it first.\n\
             Work only inside these worktrees:\n{}\n\
             Finish one repo at a time. After each repo, stop and summarize what changed so I can review it before you continue.",
            task.id,
            self.plan_path(&task.id).display(),
            wts.join("\n")
        )
    }
}

/// origin/<base> when the repo has a remote (fetched first), else the local base.
fn base_ref(dir: &Path, base: &str) -> Result<String> {
    if !has_remote(dir) {
        return Ok(base.into());
    }
    git(dir, &["fetch", "origin", base])?;
    Ok(format!("origin/{base}"))
}

fn same_path(a: &Path, b: &Path) -> bool {
    let real = |p: &Path| {
        fs::canonicalize(p)
            .or_else(|_| std::path::absolute(p))
            .unwrap_or_else(|_| p.to_path_buf())
    };
    real(a) == real(b)
}
