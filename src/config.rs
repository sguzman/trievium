use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub version: u32,
    pub entries: Vec<Entry>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub label: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub priority: i32,
}

fn is_zero(value: &i32) -> bool {
    *value == 0
}

impl Config {
    pub fn load(path: &Path) -> Result<Self> {
        let content = fs::read_to_string(path).with_context(|| {
            format!(
                "Cannot read {}. Run 'trievium init' to make a starter file.",
                path.display()
            )
        })?;
        let config: Self = serde_json::from_str(&content)
            .with_context(|| format!("Invalid JSON configuration at {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    pub fn validate(&self) -> Result<()> {
        if self.version != 1 {
            bail!(
                "Unsupported configuration version {} (expected 1)",
                self.version
            );
        }
        if self.entries.len() > 10_000 {
            bail!("Maximum 10000 entries");
        }
        let mut labels = BTreeSet::new();
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.label.trim().is_empty() {
                bail!("Entry {} has an empty label", index + 1);
            }
            if !labels.insert(&entry.label) {
                bail!(
                    "Duplicate label '{}': labels identify persistent key assignments",
                    entry.label
                );
            }
            if entry.text.is_empty() {
                bail!("Entry '{}' has no text to copy", entry.label);
            }
        }
        Ok(())
    }
}

pub fn config_path(override_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os("TRIEVIUM_CONFIG") {
        return Ok(PathBuf::from(path));
    }
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME")
        && !base.is_empty()
    {
        return Ok(PathBuf::from(base).join("trievium").join("items.json"));
    }
    let home = std::env::var_os("HOME").context("HOME or XDG_CONFIG_HOME is required")?;
    Ok(PathBuf::from(home).join(".config/trievium/items.json"))
}

/// Atomically replace a local configuration, but refuse to overwrite
/// changes made outside this editor since it loaded the file.
pub fn save_atomic(path: &Path, config: &Config, expected_file: &[u8]) -> Result<Vec<u8>> {
    config.validate()?;
    let current = fs::read(path)
        .with_context(|| format!("Reading existing configuration {}", path.display()))?;
    if current != expected_file {
        bail!("Configuration changed on disk. Reopen the editor to avoid overwriting external changes.");
    }
    let mut content = serde_json::to_vec_pretty(config)?;
    content.push(b'\n');
    let parent = path.parent().context("Configuration path has no parent directory")?;
    let name = path.file_name().context("Configuration path has no filename")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut temp_path = None;
    let result = (|| -> Result<()> {
        use std::io::Write;
        for n in 0..100 {
            let candidate = parent.join(format!(
                ".{}.{}.{}.tmp",
                name.to_string_lossy(),
                std::process::id(),
                n
            ));
            match options.open(&candidate) {
                Ok(mut file) => {
                    temp_path = Some(candidate);
                    file.write_all(&content)?;
                    file.sync_all()?;
                    // Recheck directly before committing: never silently
                    // clobber a file edited while this dialog was open.
                    if fs::read(path)? != expected_file {
                        bail!("Configuration changed on disk. Reopen the editor before saving.");
                    }
                    fs::rename(temp_path.as_ref().unwrap(), path)
                        .with_context(|| format!("Saving {}", path.display()))?;
                    return Ok(());
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error).context("Creating temporary configuration file"),
            }
        }
        bail!("No unique temporary path available for configuration save");
    })();
    if let Some(path) = temp_path {
        if result.is_err() {
            let _ = fs::remove_file(path);
        }
    }
    result?;
    Ok(content)
}

pub fn initialize(path: &Path) -> Result<()> {
    if path.exists() {
        bail!(
            "Refusing to overwrite existing configuration: {}",
            path.display()
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("Creating {}", parent.display()))?;
    }
    let example = Config {
        version: 1,
        entries: vec![
            Entry {
                label: "Greeting".into(),
                text: "Hello!".into(),
                key: Some("G".into()),
                priority: 100,
            },
            Entry {
                label: "Acknowledgement".into(),
                text: "Thank you, received.".into(),
                key: None,
                priority: 40,
            },
            Entry {
                label: "Closing".into(),
                text: "Best regards,".into(),
                key: None,
                priority: 30,
            },
        ],
    };
    // create_new avoids a race and will never overwrite a user's data.
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("Creating {}", path.display()))?;
    use std::io::Write;
    file.write_all(serde_json::to_string_pretty(&example)?.as_bytes())?;
    file.write_all(b"\n")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_empty_collection_for_in_app_management() {
        let c = Config { version: 1, entries: vec![] };
        c.validate().unwrap();
    }

    #[test]
    fn atomic_save_preserves_external_edits() {
        let dir = std::env::temp_dir().join(format!("trievium-config-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("items.json");
        let first = Config {
            version: 1,
            entries: vec![Entry {
                label: "Example".into(),
                text: "Hello!".into(),
                key: Some("E".into()),
                priority: 0,
            }],
        };
        let initial = serde_json::to_vec(&first).unwrap();
        std::fs::write(&path, &initial).unwrap();
        let updated = Config { version: 1, entries: vec![] };
        let bytes = save_atomic(&path, &updated, &initial).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(Config::load(&path).unwrap().entries.len(), 0);
        std::fs::write(&path, b"external change").unwrap();
        assert!(save_atomic(&path, &first, &bytes).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"external change");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn rejects_unsupported_version() {
        let c = Config {
            version: 2,
            entries: vec![Entry {
                label: "A".into(),
                text: "X".into(),
                key: None,
                priority: 0,
            }],
        };
        assert!(c.validate().is_err());
    }

    #[test]
    fn rejects_duplicate_labels() {
        let item = Entry {
            label: "Same".into(),
            text: "X".into(),
            key: None,
            priority: 0,
        };
        let config = Config {
            version: 1,
            entries: vec![item.clone(), item],
        };
        assert!(config.validate().is_err());
    }

    #[test]
    fn rejects_empty_text() {
        let c = Config {
            version: 1,
            entries: vec![Entry {
                label: "A".into(),
                text: String::new(),
                key: None,
                priority: 0,
            }],
        };
        assert!(c.validate().is_err());
    }
}
