use std::path::PathBuf;

pub fn install_cli() -> anyhow::Result<()> {
    let current_exe = std::env::current_exe()?;
    let user_bin = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".local")
        .join("bin");
    let hardlink = user_bin.join("proxy-manager");

    std::fs::create_dir_all(&user_bin)?;

    // Remove existing link or file
    if hardlink.exists() {
        std::fs::remove_file(&hardlink)?;
    }

    // Create hardlink
    std::fs::hard_link(&current_exe, &hardlink)?;
    println!(
        "Created hardlink: {} -> {}",
        hardlink.display(),
        current_exe.display()
    );
    println!();
    println!("See 'proxy-manager --help' for a quick start guide.");

    // Check if in PATH
    let path = std::env::var("PATH").unwrap_or_default();
    if !path.split(':').any(|p| PathBuf::from(p) == user_bin) {
        println!("NOTE: Add ~/.local/bin to your PATH:");
        println!("  export PATH=\"{}:$PATH\"", user_bin.display());
        println!("  # Add to ~/.bashrc or ~/.zshrc to persist");
    }

    Ok(())
}
