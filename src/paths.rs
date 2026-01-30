use std::path::PathBuf;

pub const DEFAULT_PORT: u16 = 8000;

pub fn config_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".to_string());
    PathBuf::from(home)
        .join(".local")
        .join("share")
        .join("proxy-manager")
}

pub fn config_file() -> PathBuf {
    config_dir().join("proxy-config.json")
}

pub fn build_dir() -> PathBuf {
    config_dir().join("build")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_paths_are_under_share() {
        let dir = config_dir();
        assert!(dir.ends_with(".local/share/proxy-manager"));
        let file = config_file();
        assert!(file.ends_with(".local/share/proxy-manager/proxy-config.json"));
        let build = build_dir();
        assert!(build.ends_with(".local/share/proxy-manager/build"));
    }
}
