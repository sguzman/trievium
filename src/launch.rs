//! Optional session-local single-instance guard for the modal picker.
//!
//! Linux file locking is advisory and the kernel releases it when the
//! last descriptor closes. No process enumeration or signal delivery is used.
use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::path::Path;

#[derive(Debug)]
pub enum Instance {
    Acquired(File),
    Busy,
}

pub fn acquire() -> Result<Option<Instance>> {
    let Some(dir) = std::env::var_os("XDG_RUNTIME_DIR").filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    Ok(Some(acquire_at(&Path::new(&dir).join("trievium.lock"))?))
}

fn acquire_at(path: &Path) -> Result<Instance> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options.open(path)
        .with_context(|| format!("Opening picker session lock at {}", path.display()))?;
    match file.try_lock() {
        Ok(()) => Ok(Instance::Acquired(file)),
        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => Ok(Instance::Busy),
        Err(error) => Err(error).with_context(|| format!("Locking {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_process_can_hold_session_lock() {
        let path = std::env::temp_dir().join(format!(
            "trievium-lock-test-{}",
            std::process::id()
        ));
        let first = acquire_at(&path).unwrap();
        assert!(matches!(first, Instance::Acquired(_)));
        assert!(matches!(acquire_at(&path).unwrap(), Instance::Busy));
        drop(first);
        assert!(matches!(acquire_at(&path).unwrap(), Instance::Acquired(_)));
        let _ = std::fs::remove_file(path);
    }
}
