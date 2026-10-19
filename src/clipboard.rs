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
        .stderr(Stdio::piped())
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
    let output = child.wait_with_output().context("Waiting for wl-copy")?;
    if !output.status.success() {
        bail!(
            "wl-copy failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        );
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

    #[test]
    fn failed_clipboard_subprocess_is_reported() {
        assert!(copy_with("false", &[], "message").is_err());
    }

    #[test]
    fn missing_clipboard_subprocess_is_reported() {
        assert!(copy_with("trievium-nonexistent-clipboard-program", &[], "message").is_err());
    }
}
