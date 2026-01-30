use crate::config::Config;
use crate::paths;
use anyhow::Result;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct Store {
    pub config_dir: PathBuf,
    pub config_file: PathBuf,
    pub build_dir: PathBuf,
}

impl Store {
    pub fn new_default() -> Result<Self> {
        let config_dir = paths::default_config_dir()?;
        let config_file = paths::default_config_file()?;
        let build_dir = paths::default_build_dir()?;
        Ok(Self {
            config_dir,
            config_file,
            build_dir,
        })
    }

    pub fn new_with_config_dir(dir: impl AsRef<Path>) -> Self {
        let config_dir = dir.as_ref().to_path_buf();
        let config_file = config_dir.join(paths::CONFIG_FILE_NAME);
        let build_dir = config_dir.join(paths::BUILD_DIR_NAME);
        Self {
            config_dir,
            config_file,
            build_dir,
        }
    }

    pub fn load(&self) -> Result<Config> {
        fs::create_dir_all(&self.config_dir)?;
        if !self.config_file.exists() {
            return Ok(Config::default());
        }

        let raw = fs::read_to_string(&self.config_file)?;
        let cfg: Config = serde_json::from_str(&raw)?;
        Ok(cfg)
    }

    pub fn save(&self, config: &Config) -> Result<()> {
        fs::create_dir_all(&self.config_dir)?;
        let raw = serde_json::to_string_pretty(config)?;
        fs::write(&self.config_file, raw)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ContainerConfig, Route};

    #[test]
    fn store_roundtrips_config() {
        let td = tempfile::tempdir().unwrap();
        let store = Store::new_with_config_dir(td.path());

        let cfg = Config {
            containers: vec![ContainerConfig {
                name: "app".to_string(),
                label: Some("Label".to_string()),
                port: Some(1234),
                network: Some("net".to_string()),
            }],
            routes: vec![Route {
                host_port: 8001,
                target: "app".to_string(),
            }],
            proxy_name: "proxy-manager".to_string(),
            network: "proxy-net".to_string(),
        };
        store.save(&cfg).unwrap();

        let loaded = store.load().unwrap();
        assert_eq!(loaded, cfg);
    }

    #[test]
    fn store_loads_default_when_missing() {
        let td = tempfile::tempdir().unwrap();
        let store = Store::new_with_config_dir(td.path());
        let loaded = store.load().unwrap();
        assert_eq!(loaded, Config::default());
    }
}
