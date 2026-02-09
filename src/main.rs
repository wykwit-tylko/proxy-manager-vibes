use clap::{Parser, Subcommand};
use proxy_manager_rs::config::Paths;
use proxy_manager_rs::docker::DockerCli;
use proxy_manager_rs::manager::ProxyManager;
use proxy_manager_rs::tui::run_tui;

#[derive(Debug, Parser)]
#[command(
    name = "proxy-manager",
    about = "Manage Nginx proxy routes for Docker containers"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Debug, Subcommand)]
enum Commands {
    Start,
    Stop {
        port: Option<u16>,
    },
    Restart,
    Reload,
    List,
    Networks,
    Status,
    Config,
    Install,
    Detect {
        filter: Option<String>,
    },
    Logs {
        #[arg(short, long)]
        follow: bool,
        #[arg(short = 'n', long, default_value_t = 100)]
        tail: usize,
    },
    Add {
        container: String,
        label: Option<String>,
        #[arg(short, long)]
        port: Option<u16>,
        #[arg(short, long)]
        network: Option<String>,
    },
    Remove {
        identifier: String,
    },
    Switch {
        identifier: String,
        port: Option<u16>,
    },
    Tui,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let paths = Paths::from_env()?;
    let manager = ProxyManager::new(DockerCli, paths);

    match cli.command {
        None => {
            println!("Run `proxy-manager --help` for usage.");
        }
        Some(Commands::Start) => println!("{}", manager.start_proxy()?),
        Some(Commands::Stop { port: Some(port) }) => println!("{}", manager.stop_port(port)?),
        Some(Commands::Stop { port: None }) => println!("{}", manager.stop_proxy()?),
        Some(Commands::Restart) => println!("{}", manager.restart_proxy()?),
        Some(Commands::Reload) => println!("{}", manager.reload_proxy()?),
        Some(Commands::List) => println!("{}", manager.list_containers_output()?),
        Some(Commands::Networks) => println!("{}", manager.list_networks_output()?),
        Some(Commands::Status) => println!("{}", manager.status_output()?),
        Some(Commands::Config) => println!("{}", manager.show_config_output()?),
        Some(Commands::Install) => {
            let exe = std::env::current_exe()?;
            println!("{}", manager.install_cli(&exe)?);
        }
        Some(Commands::Detect { filter }) => {
            println!("{}", manager.detect_containers_output(filter.as_deref())?)
        }
        Some(Commands::Logs { follow, tail }) => manager.show_logs(follow, tail)?,
        Some(Commands::Add {
            container,
            label,
            port,
            network,
        }) => println!(
            "{}",
            manager.add_container(&container, label.as_deref(), port, network.as_deref())?
        ),
        Some(Commands::Remove { identifier }) => {
            println!("{}", manager.remove_container(&identifier)?)
        }
        Some(Commands::Switch { identifier, port }) => {
            println!("{}", manager.switch_target(&identifier, port)?)
        }
        Some(Commands::Tui) => run_tui(&manager)?,
    }

    Ok(())
}
