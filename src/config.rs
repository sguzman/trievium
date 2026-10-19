use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
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
        if self.entries.is_empty() {
            bail!("At least one entry is required");
        }
        if self.entries.len() > 10_000 {
            bail!("Maximum 10000 entries");
        }
        for (index, entry) in self.entries.iter().enumerate() {
            if entry.label.trim().is_empty() {
                bail!("Entry {} has an empty label", index + 1);
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
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
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
