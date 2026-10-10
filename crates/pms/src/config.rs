use std::fs;
use std::path::PathBuf;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

pub const DEFAULT_API_URL: &str = "https://api.pmservers.org";

/// `PMS_CONFIG_DIR`, else `~/.config/pms` on every OS (the plan fixes this path).
pub fn config_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("PMS_CONFIG_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let home = dirs::home_dir().context("cannot find the home directory")?;
    Ok(home.join(".config").join("pms"))
}

/// `PMS_DATA_DIR`, else the OS data folder (`~/.local/share/pms` on Linux).
pub fn data_dir() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("PMS_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let base = dirs::data_dir().context("cannot find the data directory")?;
    Ok(base.join("pms"))
}

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Config {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

impl Config {
    fn path() -> Result<PathBuf> {
        Ok(config_dir()?.join("config.toml"))
    }

    pub fn load() -> Result<Self> {
        let path = Self::path()?;
        match fs::read_to_string(&path) {
            Ok(text) => {
                toml::from_str(&text).with_context(|| format!("bad config {}", path.display()))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e).with_context(|| format!("cannot read {}", path.display())),
        }
    }

    pub fn save(&self) -> Result<()> {
        let path = Self::path()?;
        fs::create_dir_all(path.parent().expect("has parent"))?;
        fs::write(&path, toml::to_string_pretty(self)?)
            .with_context(|| format!("cannot write {}", path.display()))
    }
}
