//! End-to-end tests against throwaway git repos with a fake remote.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use tempfile::TempDir;

/// A workspace with two repos, each cloned from a bare "origin" with master and a staging branch.
struct Env {
    _root: TempDir,
    root: PathBuf,
    ws: PathBuf,
}

const GIT_ENV: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "t"),
    ("GIT_AUTHOR_EMAIL", "t@t"),
    ("GIT_COMMITTER_NAME", "t"),
    ("GIT_COMMITTER_EMAIL", "t@t"),
];

/// Runs a command; panics with stderr on failure, returns trimmed stdout.
fn run(cmd: &mut Command) -> String {
    let out = cmd.envs(GIT_ENV).output().unwrap();
    assert!(
        out.status.success(),
        "{:?} failed:\n{}",
        cmd,
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn git(cwd: &Path, args: &[&str]) -> String {
    run(Command::new("git").args(args).current_dir(cwd))
}

impl Env {
    fn setup() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = fs::canonicalize(dir.path()).unwrap();
        let ws = root.join("ws");
        fs::create_dir(&ws).unwrap();
        for repo in ["core-service", "deposit-engine"] {
            let bare = root.join("remotes").join(format!("{repo}.git"));
            git(
                &root,
                &["init", "--bare", "-b", "master", bare.to_str().unwrap()],
            );
            let dir = ws.join(repo);
            git(
                &root,
                &["clone", bare.to_str().unwrap(), dir.to_str().unwrap()],
            );
            fs::write(dir.join("a.txt"), "base\n").unwrap();
            git(&dir, &["add", "."]);
            git(&dir, &["commit", "-m", "base"]);
            git(&dir, &["push", "origin", "master"]);
            git(&dir, &["push", "origin", "master:testing/sprint-a"]);
        }
        let env = Env {
            _root: dir,
            root,
            ws,
        };
        env.whet(&[
            "init",
            "--workspace",
            env.ws.to_str().unwrap(),
            "--staging",
            "testing/sprint-a",
        ]);
        env
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_whet"));
        cmd.args(args).env("WHET_HOME", self.root.join("home"));
        cmd
    }

    fn whet(&self, args: &[&str]) -> String {
        run(&mut self.cmd(args))
    }

    /// Runs whet expecting failure; returns stderr.
    fn whet_fails(&self, args: &[&str]) -> String {
        let out = self.cmd(args).output().unwrap();
        assert!(!out.status.success(), "whet {args:?} should have failed");
        String::from_utf8_lossy(&out.stderr).into_owned()
    }

    fn commit(&self, dir: &Path, file: &str, msg: &str) {
        fs::write(dir.join(file), format!("{msg}\n")).unwrap();
        git(dir, &["add", "."]);
        git(dir, &["commit", "-m", msg]);
    }
}

#[test]
fn task_new_creates_a_worktree_per_repo_and_a_plan() {
    let env = Env::setup();
    env.whet(&["task", "new", "INS-1", "core-service", "deposit-engine"]);
    for repo in ["core-service", "deposit-engine"] {
        let wt = env.ws.join(format!("{repo}-worktree")).join("INS-1");
        assert_eq!(git(&wt, &["branch", "--show-current"]), "INS-1");
    }
    let plan = fs::read_to_string(env.ws.join("my-plans/INS-1/plan.md")).unwrap();
    assert!(plan.starts_with("# INS-1"));
    let list = env.whet(&["task", "list"]);
    assert!(
        list.contains("INS-1") && list.contains(&format!("{:<24} 0 commit(s)", "core-service")),
        "{list}"
    );
}

#[test]
fn ship_cherry_picks_only_new_task_commits_onto_staging_and_is_repeatable() {
    let env = Env::setup();
    env.whet(&["task", "new", "INS-2", "core-service"]);
    let wt = env.ws.join("core-service-worktree/INS-2");
    env.commit(&wt, "b.txt", "feature one");

    assert!(
        env.whet(&["task", "ship", "INS-2"])
            .contains("1 commit(s) onto testing/sprint-a")
    );
    env.commit(&wt, "c.txt", "feature two");
    assert!(
        env.whet(&["task", "ship", "INS-2"])
            .contains("1 commit(s) onto testing/sprint-a")
    );
    assert!(env.whet(&["task", "ship", "INS-2"]).contains("nothing new"));

    let staging = env.ws.join("core-service-worktree/_staging");
    let log = git(
        &staging,
        &["log", "--format=%s", "-2", "origin/testing/sprint-a"],
    );
    // Commit subjects may carry a prefix from the user's git hooks, so match loosely.
    let subjects: Vec<&str> = log.lines().collect();
    assert!(
        subjects[0].contains("feature two") && subjects[1].ends_with("feature one"),
        "{log}"
    );
}

#[test]
fn ship_refuses_when_a_task_worktree_holds_the_staging_branch() {
    let env = Env::setup();
    let dir = env.ws.join("core-service");
    let stray = env.ws.join("stray");
    git(
        &dir,
        &[
            "worktree",
            "add",
            "--track",
            "-b",
            "testing/sprint-a",
            stray.to_str().unwrap(),
            "origin/testing/sprint-a",
        ],
    );
    env.whet(&["task", "new", "INS-3", "core-service"]);
    let err = env.whet_fails(&["task", "ship", "INS-3"]);
    assert!(
        err.contains("checked out in") && err.contains("stray") && err.contains("switch --detach"),
        "{err}"
    );
}

#[test]
fn close_removes_worktrees_but_refuses_uncommitted_work() {
    let env = Env::setup();
    env.whet(&["task", "new", "INS-4", "core-service"]);
    let wt = env.ws.join("core-service-worktree/INS-4");
    fs::write(wt.join("wip.txt"), "wip").unwrap();
    assert!(
        env.whet_fails(&["task", "close", "INS-4"])
            .contains("uncommitted changes")
    );
    env.whet(&["task", "close", "INS-4", "--force"]);
    assert!(!wt.exists());
    assert!(env.whet(&["task", "list"]).contains("No active tasks"));
}

#[test]
fn install_claude_replaces_old_whet_hooks_and_keeps_others() {
    let env = Env::setup();
    let home = env.root.join("user-home");
    let claude = home.join(".claude");
    fs::create_dir_all(&claude).unwrap();
    fs::write(claude.join("CLAUDE.md"), "- existing rule\n").unwrap();
    fs::write(
        claude.join("settings.json"),
        r#"{
  "model": "opus",
  "hooks": {
    "SessionEnd": [
      { "hooks": [{ "type": "command", "command": "\"/x/bun\" \"/x/whet/src/cli.ts\" learn harvest --from claude" }] },
      { "hooks": [{ "type": "command", "command": "say done" }] }
    ]
  }
}"#,
    )
    .unwrap();

    for _ in 0..2 {
        run(env.cmd(&["install", "claude"]).env("HOME", &home));
    }

    let settings: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(claude.join("settings.json")).unwrap()).unwrap();
    assert_eq!(settings["model"], "opus");
    let end: Vec<&str> = settings["hooks"]["SessionEnd"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["hooks"][0]["command"].as_str().unwrap())
        .collect();
    assert_eq!(end.len(), 2, "{end:?}");
    assert_eq!(end[0], "say done");
    assert!(
        end[1].ends_with("whet\" learn harvest --from claude"),
        "{}",
        end[1]
    );

    assert_eq!(
        fs::read_link(claude.join("CLAUDE.md")).unwrap(),
        env.root.join("home/rules.md")
    );
    assert_eq!(
        env.whet(&["rules"]).lines().next().unwrap(),
        "- existing rule"
    );
    assert!(claude.join("skills/wrap-up/SKILL.md").exists());
}
