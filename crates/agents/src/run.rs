use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};
use std::thread;

use anyhow::{Context, Result, bail};

/// Runs a command to completion, feeding `input` on stdin, and returns stdout.
pub fn capture(cmd: &[&str], input: &str) -> Result<String> {
    let (program, args) = cmd.split_first().context("empty command")?;
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("could not run {program}. Is it installed?"))?;
    // Write stdin on its own thread so a child that answers early can't deadlock us.
    let mut stdin = child.stdin.take().context("no stdin")?;
    let input = input.to_owned();
    let writer = thread::spawn(move || stdin.write_all(input.as_bytes()));
    let out = child.wait_with_output()?;
    let _ = writer.join();
    if !out.status.success() {
        bail!(
            "{program} failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// Runs an interactive command attached to this terminal.
pub fn interactive(cmd: &[&str], cwd: &Path) -> Result<()> {
    let (program, args) = cmd.split_first().context("empty command")?;
    Command::new(program)
        .args(args)
        .current_dir(cwd)
        .status()
        .with_context(|| format!("could not run {program}. Is it installed?"))?;
    Ok(())
}
