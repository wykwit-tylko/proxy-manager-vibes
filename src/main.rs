mod cli;
mod config;
mod docker;
mod nginx;
mod proxy;
mod tui;

use anyhow::Result;
use clap::Parser;

use cli::{Cli, Command};

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    let Some(command) = cli.command else {
        // No subcommand: print help
        use clap::CommandFactory;
        Cli::command().print_help()?;
        println!();
        return Ok(());
    };

    match command {
        Command::Tui => {
            tui::run().await?;
        }
        _ => {
            run_cli_command(command).await?;
        }
    }

    Ok(())
}

async fn run_cli_command(command: Command) -> Result<()> {
    let docker_client = docker::create_client()?;
    let mut config = config::load_config()?;

    match command {
        Command::Start => {
            proxy::start_proxy(&docker_client, &config).await?;
        }
        Command::Stop { port } => {
            if let Some(port) = port {
                proxy::stop_port(&docker_client, &mut config, port).await?;
            } else {
                proxy::stop_proxy(&docker_client, &config).await?;
            }
        }
        Command::Restart => {
            proxy::stop_proxy(&docker_client, &config).await?;
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
            proxy::start_proxy(&docker_client, &config).await?;
        }
        Command::Reload => {
            proxy::reload_proxy(&docker_client, &config).await?;
        }
        Command::List => {
            proxy::list_containers(&config);
        }
        Command::Networks => {
            proxy::show_networks(&docker_client).await?;
        }
        Command::Status => {
            proxy::show_status(&docker_client, &config).await?;
        }
        Command::Config => {
            proxy::show_config(&config)?;
        }
        Command::Logs { tail } => {
            proxy::show_logs(&docker_client, &config, tail).await?;
        }
        Command::Add {
            container,
            label,
            port,
            network,
        } => {
            proxy::add_container(
                &docker_client,
                &mut config,
                &container,
                label.as_deref(),
                port,
                network.as_deref(),
            )
            .await?;
        }
        Command::Remove { identifier } => {
            proxy::remove_container(&mut config, &identifier)?;
        }
        Command::Switch { identifier, port } => {
            proxy::switch_target(&docker_client, &mut config, &identifier, port).await?;
        }
        Command::Detect { filter } => {
            proxy::detect_containers(&docker_client, filter.as_deref()).await?;
        }
        Command::Tui => unreachable!(),
    }

    Ok(())
}
