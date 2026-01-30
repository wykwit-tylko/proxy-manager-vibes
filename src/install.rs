use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InstallOutcome {
    pub link_path: PathBuf,
    pub target_path: PathBuf,
    pub path_notice: Option<String>,
}

pub fn install_hardlink(binary_path: &Path, user_bin: &Path) -> Result<InstallOutcome> {
    fs::create_dir_all(user_bin).context("create ~/.local/bin")?;
    let link_path = user_bin.join("proxy-manager");

    if link_path.exists() {
        fs::remove_file(&link_path).context("remove existing link")?;
    }

    fs::hard_link(binary_path, &link_path).context("create hardlink")?;

    let path_env = env::var("PATH").unwrap_or_default();
    let notice = if !path_env
        .split(':')
        .any(|p| !p.is_empty() && Path::new(p) == user_bin)
    {
        Some(format!(
            "NOTE: Add {} to your PATH:\n  export PATH=\"{}:$PATH\"\n  # Add to ~/.bashrc or ~/.zshrc to persist",
            user_bin.display(),
            user_bin.display()
        ))
    } else {
        None
    };

    Ok(InstallOutcome {
        link_path,
        target_path: binary_path.to_path_buf(),
        path_notice: notice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn install_creates_hardlink_in_target_dir() {
        let td = tempfile::tempdir().unwrap();
        let bin_dir = td.path().join("bin");
        let exe_path = td.path().join("proxy-manager");
        fs::write(&exe_path, "fake").unwrap();

        let out = install_hardlink(&exe_path, &bin_dir).unwrap();
        assert_eq!(out.link_path, bin_dir.join("proxy-manager"));
        assert!(out.link_path.exists());
    }
}
