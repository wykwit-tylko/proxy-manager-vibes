use anyhow::{Result, anyhow};
use directories::BaseDirs;
use std::path::PathBuf;

pub const APP_DIR_NAME: &str = "proxy-manager";
pub const CONFIG_FILE_NAME: &str = "proxy-config.json";
pub const BUILD_DIR_NAME: &str = "build";

pub fn default_config_dir() -> Result<PathBuf> {
    let base = BaseDirs::new().ok_or_else(|| anyhow!("Could not determine user directories"))?;
    Ok(base.data_dir().join(APP_DIR_NAME))
}

pub fn default_config_file() -> Result<PathBuf> {
    Ok(default_config_dir()?.join(CONFIG_FILE_NAME))
}

pub fn default_build_dir() -> Result<PathBuf> {
    Ok(default_config_dir()?.join(BUILD_DIR_NAME))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_paths_are_consistent() {
        let dir = default_config_dir().unwrap();
        let file = default_config_file().unwrap();
        let build = default_build_dir().unwrap();

        assert!(file.starts_with(&dir));
        assert!(build.starts_with(&dir));
        assert!(file.ends_with(CONFIG_FILE_NAME));
    }
}
