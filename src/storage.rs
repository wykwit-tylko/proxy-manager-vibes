use std::fs;
use std::path::Path;

use anyhow::{Context, Result};

use crate::config::AppConfig;
use crate::paths::config_file;

pub fn load_config() -> Result<AppConfig> {
    load_config_from(&config_file())
}

pub fn save_config(config: &AppConfig) -> Result<()> {
    save_config_to(&config_file(), config)
}

pub fn load_config_from(path: &Path) -> Result<AppConfig> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }

    let contents = fs::read_to_string(path)
        .with_context(|| format!("Failed to read config file {}", path.display()))?;
    let config: AppConfig = serde_json::from_str(&contents)
        .with_context(|| format!("Invalid config JSON in {}", path.display()))?;
    Ok(config)
}

pub fn save_config_to(path: &Path, config: &AppConfig) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("Failed to create config dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(config)?;
    fs::write(path, json)
        .with_context(|| format!("Failed to write config file {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn load_returns_default_when_missing() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("missing.json");
        let config = load_config_from(&path).expect("load config");
        assert_eq!(config, AppConfig::default());
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = tempdir().expect("tempdir");
        let path = dir.path().join("proxy-config.json");
        let config = AppConfig::default();

        save_config_to(&path, &config).expect("save config");
        let loaded = load_config_from(&path).expect("load config");
        assert_eq!(loaded, config);
    }
}
