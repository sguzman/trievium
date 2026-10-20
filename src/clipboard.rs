//! wl-copy forks by default and stays alive as the clipboard data source after
//! Trievium exits. Do not pass --foreground or --paste-once.
use anyhow::{Context, Result, bail};
use std::io::Write;
use std::process::{Command, Stdio};

pub fn copy_text(text: &str) -> Result<()> {
    copy_with("wl-copy", &["--type", "text/plain;charset=utf-8"], text)
}

/// The real command uses wl-copy. An injectable subprocess boundary allows
/// lossless UTF-8 verification in headless CI without touching any clipboard.
fn copy_with(program: &str, args: &[&str], text: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        // wl-copy forks a background clipboard owner. If stderr is a pipe,
        // the daemon can inherit that pipe and wait_with_output() will block
        // forever waiting for EOF even after the launcher has exited.
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "Could not start {program}. Install wl-clipboard and run under a Wayland session"
            )
        })?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .context("Writing clipboard content")?;
    }
    // Wait only for the direct launcher process, not pipes inherited by
    // its background clipboard-serving child.
    let status = child.wait().context("Waiting for clipboard launcher")?;
    if !status.success() {
        bail!("Clipboard launcher exited with {status}");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_multiline_unicode_payload() {
        let path = std::env::temp_dir().join(format!(
            "trievium-clipboard-roundtrip-{}",
            std::process::id()
        ));
        let destination = path.to_str().expect("temporary directory is UTF-8");
        let original = "Español: á é í ó ú — 汉字 🚀\nSecond line\n\n";
        // tee copies standard input byte for byte, making the clipboard
        // boundary testable without a running Wayland compositor.
        copy_with("tee", &[destination], original).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original.as_bytes());
        std::fs::remove_file(path).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn does_not_block_on_clipboard_daemon_inherited_stderr() {
        // The direct child consumes stdin and forks a descendant that keeps
        // inherited descriptors open for several seconds, as wl-copy does.
        // wait_with_output() with piped stderr would stall here until EOF.
        let start = std::time::Instant::now();
        copy_with("sh", &["-c", "cat >/dev/null; sleep 3 &"], "example").unwrap();
        assert!(
            start.elapsed() < std::time::Duration::from_secs(1),
            "clipboard handoff waited for background server"
        );
    }

    #[test]
    fn failed_clipboard_subprocess_is_reported() {
        assert!(copy_with("false", &[], "message").is_err());
    }

    #[test]
    fn missing_clipboard_subprocess_is_reported() {
        assert!(copy_with("trievium-nonexistent-clipboard-program", &[], "message").is_err());
    }
}
