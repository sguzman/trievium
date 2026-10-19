//! wl-copy forks by default and stays alive as the clipboard data source after
//! Trievium exits. Do not pass --foreground or --paste-once.
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy_text(text: &str) -> Result<()> {
    let mut child = Command::new("wl-copy")
        .args(["--type", "text/plain;charset=utf-8"])
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not start wl-copy. Install wl-clipboard and run under a Wayland session")?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(text.as_bytes()).context("Writing clipboard content")?;
    }
    let output = child.wait_with_output().context("Waiting for wl-copy")?;
    if !output.status.success() {
        bail!("wl-copy failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}
