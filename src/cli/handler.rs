use crate::cli::{Cli, Commands};
use crate::proxy::ProxyManager;
use crate::utils::install_cli;
use clap::Parser;

pub async fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Some(command) => match command {
            Commands::Start => {
                let manager = ProxyManager::new()?;
                manager.start().await?;
            }
            Commands::Stop { port } => {
                let manager = ProxyManager::new()?;
                if let Some(p) = port {
                    manager.stop_port(p).await?;
                } else {
                    manager.stop().await?;
                }
            }
            Commands::Restart => {
                let manager = ProxyManager::new()?;
                manager.restart().await?;
            }
            Commands::Reload => {
                let manager = ProxyManager::new()?;
                manager.reload().await?;
            }
            Commands::List => {
                let manager = ProxyManager::new()?;
                manager.list_containers().await?;
            }
            Commands::Networks => {
                let manager = ProxyManager::new()?;
                manager.list_networks().await?;
            }
            Commands::Status => {
                let manager = ProxyManager::new()?;
                manager.status().await?;
            }
            Commands::Install => {
                install_cli()?;
            }
            Commands::Config => {
                ProxyManager::show_config()?;
            }
            Commands::Logs { follow, tail } => {
                let manager = ProxyManager::new()?;
                manager.show_logs(tail, follow).await?;
            }
            Commands::Add {
                container,
                label,
                port,
                network,
            } => {
                let manager = ProxyManager::new()?;
                manager
                    .add_container(container, label, port, network)
                    .await?;
            }
            Commands::Remove { identifier } => {
                let manager = ProxyManager::new()?;
                manager.remove_container(&identifier).await?;
            }
            Commands::Switch { identifier, port } => {
                let manager = ProxyManager::new()?;
                manager.switch_target(&identifier, port).await?;
            }
            Commands::Detect { filter } => {
                let manager = ProxyManager::new()?;
                manager.detect_containers(filter.as_deref()).await?;
            }
            Commands::Tui => {
                crate::tui::run_tui().await?;
            }
        },
        None => {
            // Print help if no command given
            Cli::parse_from(["proxy-manager", "--help"]);
        }
    }

    Ok(())
}
