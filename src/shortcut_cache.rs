//! Local, best-effort persistence of automatic mnemonic assignments.
//! Never store actual clipboard payloads here.
use crate::config::Entry;
use crate::trie::Keymap;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Serialize)]
struct Cache {
    version: u32,
    keys: BTreeMap<String, String>,
}

pub fn path_for_config(config_path: &Path) -> Result<PathBuf> {
    let root = if let Some(path) = std::env::var_os("XDG_STATE_HOME")
        && !path.is_empty()
    {
        PathBuf::from(path)
    } else {
        let home = std::env::var_os("HOME").context("HOME or XDG_STATE_HOME is required")?;
        PathBuf::from(home).join(".local/state")
    };
    let canonical = fs::canonicalize(config_path).unwrap_or_else(|_| config_path.to_path_buf());
    // Stable, non-cryptographic path identifier; separate independent configs.
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in canonical.as_os_str().to_string_lossy().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    Ok(root.join("trievium").join(format!("keys-{hash:016x}.json")))
}

pub fn load(path: &Path) -> BTreeMap<String, String> {
    match fs::read(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
        Err(error) => {
            eprintln!(
                "trievium: cannot read shortcut cache at {}: {error}",
                path.display()
            );
            BTreeMap::new()
        }
        Ok(data) => match serde_json::from_slice::<Cache>(&data) {
            Ok(cache) if cache.version == 1 => cache.keys,
            _ => {
                eprintln!(
                    "trievium: ignoring unsupported/corrupt shortcut cache at {}",
                    path.display()
                );
                BTreeMap::new()
            }
        },
    }
}

pub fn save(path: &Path, entries: &[Entry], map: &Keymap) -> Result<()> {
    let data = Cache {
        version: 1,
        keys: entries
            .iter()
            .enumerate()
            .filter(|(_, entry)| entry.key.is_none())
            .map(|(index, entry)| (entry.label.clone(), map.key(index).to_owned()))
            .collect(),
    };
    let contents = serde_json::to_vec_pretty(&data)?;
    let parent = path
        .parent()
        .context("Shortcut cache has no parent directory")?;
    fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
    let temporary = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| -> Result<()> {
        let mut options = fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        use std::io::Write;
        let mut file = options
            .open(&temporary)
            .with_context(|| format!("Creating {}", temporary.display()))?;
        file.write_all(&contents)?;
        file.sync_all()?;
        fs::rename(&temporary, path).with_context(|| format!("Saving {}", path.display()))?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_cache_is_empty() {
        let path = std::env::temp_dir().join(format!(
            "trievium-missing-cache-{}.json",
            std::process::id()
        ));
        assert!(load(&path).is_empty());
    }

    #[test]
    fn malformed_cache_is_ignored() {
        let parsed: Result<Cache, _> = serde_json::from_slice(br#"{"version":1,"keys":["wrong"]}"#);
        assert!(parsed.is_err());
    }
}
