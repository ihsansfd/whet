use std::fs;
use std::io::{IsTerminal, Read};
use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::Value;
use whet_core::agent::Agent;
use whet_core::config::{AgentName, Config, Paths, expand_home};
use whet_core::learn::{pending_count, review_prompt, run_harvest, start_harvest};
use whet_core::rules::{RULES_LINE_CAP, line_count, read_rules};
use whet_core::tasks::Tasks;

/// whet: sharpen your AI coding agent
#[derive(Parser)]
#[command(name = "whet", version)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Set up ~/.whet
    Init {
        /// Folder holding the main checkout of each repo
        #[arg(long)]
        workspace: String,
        /// Branch that new tasks start from [default: master]
        #[arg(long)]
        base: Option<String>,
        /// Branch that deploys to staging
        #[arg(long)]
        staging: Option<String>,
        /// Agent used by `task run` and `learn review` [default: claude]
        #[arg(long)]
        agent: Option<AgentName>,
    },
    /// Wire rules, harvesting and /wrap-up into that tool (claude or codex)
    Install { agent: AgentName },
    /// Show or change settings
    Config {
        #[command(subcommand)]
        op: Option<ConfigOp>,
    },
    /// Show the rules every tool reads
    Rules,
    /// Turn corrections into rules
    Learn {
        #[command(subcommand)]
        cmd: LearnCmd,
    },
    /// Multi-repo tasks: worktrees, plan, staging
    Task {
        #[command(subcommand)]
        cmd: TaskCmd,
    },
}

#[derive(Subcommand)]
enum ConfigOp {
    Get { key: String },
    Set { key: String, value: String },
}

#[derive(Subcommand)]
enum LearnCmd {
    /// Turn corrections into rules, with your approval
    Review {
        #[arg(long)]
        from: Option<AgentName>,
    },
    /// How many candidate rules are waiting
    Pending,
    /// Print the review instructions (used by /wrap-up)
    #[command(hide = true)]
    Prompt,
    /// Session-end hook: harvest corrections in the background
    #[command(hide = true)]
    Harvest {
        #[arg(long)]
        from: Option<AgentName>,
        #[arg(long)]
        transcript: Option<PathBuf>,
    },
    /// The background half of `harvest`
    #[command(hide = true)]
    HarvestRun {
        #[arg(long, default_value = "claude")]
        from: AgentName,
        #[arg(long)]
        input: PathBuf,
        #[arg(long)]
        label: String,
    },
    /// Session-start hook: say when candidates are waiting
    #[command(hide = true)]
    Notice {
        #[arg(long, default_value = "claude")]
        from: AgentName,
    },
}

#[derive(Subcommand)]
enum TaskCmd {
    /// Worktree per repo + plan, from the base branch
    New {
        id: String,
        #[arg(required = true)]
        repos: Vec<String>,
        #[arg(long)]
        base: Option<String>,
    },
    /// Active tasks with commits ahead and uncommitted changes per repo
    List,
    /// Start your agent on the task's worktrees
    Run {
        id: String,
        #[arg(long)]
        agent: Option<AgentName>,
    },
    /// Cherry-pick new commits onto staging
    Ship {
        id: String,
        #[arg(long)]
        to: Option<String>,
        #[arg(long)]
        no_push: bool,
    },
    /// Remove the worktrees (branches are kept)
    Close {
        id: String,
        #[arg(long)]
        force: bool,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli.cmd, &Paths::from_env()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e:#}");
            ExitCode::FAILURE
        }
    }
}

/// This binary's absolute path, so hooks and the detached harvester work without PATH.
fn whet_exe() -> Result<PathBuf> {
    std::env::current_exe().context("could not locate the whet binary")
}

fn agent(name: AgentName) -> Result<Box<dyn Agent>> {
    Ok(whet_agents::for_name(
        name,
        &format!("\"{}\"", whet_exe()?.display()),
    ))
}

fn print(lines: &[String]) {
    for l in lines {
        println!("{l}");
    }
}

fn run(cmd: Cmd, paths: &Paths) -> Result<()> {
    match cmd {
        Cmd::Init {
            workspace,
            base,
            staging,
            agent,
        } => {
            let mut cfg = Config::new(expand_home(&workspace));
            if let Some(b) = base {
                cfg.base_branch = b;
            }
            cfg.staging_branch = staging;
            if let Some(a) = agent {
                cfg.agent = a;
            }
            cfg.save(paths)?;
            fs::create_dir_all(&paths.tasks)?;
            if !paths.rules.exists() {
                fs::write(&paths.rules, "")?;
            }
            println!("whet home: {}", paths.home.display());
            println!("workspace: {}", cfg.workspace.display());
            println!("next: whet install {}", cfg.agent);
        }
        Cmd::Install { agent: name } => {
            Config::load(paths)?;
            print(&agent(name)?.install(paths)?);
        }
        Cmd::Config { op } => {
            let mut cfg = Config::load(paths)?;
            match op {
                None => println!("{}", serde_json::to_string_pretty(&cfg)?),
                Some(ConfigOp::Get { key }) => println!("{}", cfg.get(&key)?),
                Some(ConfigOp::Set { key, value }) => {
                    cfg.set(&key, &value)?;
                    cfg.save(paths)?;
                    println!("{key} = {value}");
                }
            }
        }
        Cmd::Rules => {
            let rules = read_rules(paths)?;
            println!(
                "{}",
                if rules.is_empty() {
                    "(no rules yet)"
                } else {
                    &rules
                }
            );
            println!(
                "\n{}: {}/{RULES_LINE_CAP} lines",
                paths.rules.display(),
                line_count(&rules)
            );
        }
        Cmd::Learn { cmd } => learn(cmd, paths)?,
        Cmd::Task { cmd } => task(cmd, paths)?,
    }
    Ok(())
}

fn learn(cmd: LearnCmd, paths: &Paths) -> Result<()> {
    match cmd {
        LearnCmd::Harvest { from, transcript } => {
            // Session-end hook. Must never fail loudly: it runs as the harness exits,
            // and a failed harvest only costs one session's candidates.
            let _ = harvest(paths, from, transcript);
        }
        LearnCmd::HarvestRun { from, input, label } => {
            run_harvest(agent(from)?.as_ref(), paths, &input, &label)?;
        }
        LearnCmd::Notice { from } => {
            let n = pending_count(paths)?;
            if n > 0
                && let Some(notice) = agent(from)?.pending_notice(n)
            {
                println!("{notice}");
            }
        }
        LearnCmd::Pending => println!(
            "{} candidate rule(s) in {}",
            pending_count(paths)?,
            paths.inbox.display()
        ),
        LearnCmd::Prompt => println!("{}", review_prompt(paths)),
        LearnCmd::Review { from } => {
            let cfg = Config::load(paths)?;
            agent(from.unwrap_or(cfg.agent))?.launch(&cfg.workspace, &review_prompt(paths))?;
        }
    }
    Ok(())
}

fn harvest(paths: &Paths, from: Option<AgentName>, transcript: Option<PathBuf>) -> Result<()> {
    let hook = read_stdin_json()?;
    let field = |k: &str| hook[k].as_str().map(str::to_owned);
    let Some(transcript) = transcript.or_else(|| field("transcript_path").map(PathBuf::from))
    else {
        return Ok(());
    };
    let cwd = field("cwd").unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_default()
            .display()
            .to_string()
    });
    let label = format!(
        "{cwd} · session {}",
        field("session_id").as_deref().unwrap_or("?")
    );
    let name = match from {
        Some(n) => n,
        None => Config::load(paths)?.agent,
    };
    start_harvest(agent(name)?.as_ref(), &transcript, &label, &whet_exe()?)?;
    Ok(())
}

/// The JSON a harness pipes into a hook, or null when run from a terminal.
fn read_stdin_json() -> Result<Value> {
    let mut stdin = std::io::stdin();
    if stdin.is_terminal() {
        return Ok(Value::Null);
    }
    let mut raw = String::new();
    stdin.read_to_string(&mut raw)?;
    if raw.trim().is_empty() {
        return Ok(Value::Null);
    }
    Ok(serde_json::from_str(&raw)?)
}

fn task(cmd: TaskCmd, paths: &Paths) -> Result<()> {
    let cfg = Config::load(paths)?;
    let default_agent = cfg.agent;
    let workspace = cfg.workspace.clone();
    let tasks = Tasks::new(cfg, paths);
    match cmd {
        TaskCmd::New { id, repos, base } => {
            print(&tasks.create(&id, &repos, base.as_deref())?);
            println!("next: fill in the plan, then `whet task run {id}`");
        }
        TaskCmd::List => {
            let active = tasks.active()?;
            if active.is_empty() {
                println!("No active tasks.");
            }
            for t in &active {
                println!("{}  (plan: {})", t.id, tasks.plan_path(&t.id).display());
                for s in tasks.status(t)? {
                    let state = if !s.exists {
                        "missing".to_string()
                    } else {
                        format!(
                            "{} commit(s){}",
                            s.ahead,
                            if s.dirty { ", uncommitted changes" } else { "" }
                        )
                    };
                    println!("  {:<24} {state}", s.repo);
                }
            }
        }
        TaskCmd::Run { id, agent: name } => {
            let t = tasks.load(&id)?;
            agent(name.unwrap_or(default_agent))?.launch(&workspace, &tasks.run_prompt(&t))?;
        }
        TaskCmd::Ship { id, to, no_push } => {
            let t = tasks.load(&id)?;
            print(&tasks.ship(&t, to.as_deref(), !no_push)?);
        }
        TaskCmd::Close { id, force } => {
            let t = tasks.load(&id)?;
            print(&tasks.close(&t, force)?);
            println!("tip: `whet learn review` turns this task's corrections into rules");
        }
    }
    Ok(())
}
