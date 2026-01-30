use anyhow::Result;
use clap::{Parser, Subcommand};
use proxy_manager::app::App;
use proxy_manager::docker::BollardDocker;
use proxy_manager::install;
use proxy_manager::store::Store;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(
    name = "proxy-manager",
    about = "Manage Nginx proxy to route multiple ports to different docker app containers.",
    after_help = "Quick Start:\n  # 1. Add containers\n  proxy-manager add my-app-v1 Foo -p 8000\n  proxy-manager add my-app-v2 Bar -p 8080\n\n  # 2. Switch ports to containers (adds routes)\n  proxy-manager switch my-app-v1 8000\n  proxy-manager switch my-app-v2 8001\n\n  # 3. Start the proxy (routes multiple ports)\n  proxy-manager start\n\n  # 4. View status\n  proxy-manager status\n"
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
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
    Logs {
        #[arg(short = 'f', long = "follow")]
        follow: bool,
        #[arg(short = 'n', long = "tail", default_value_t = 100)]
        tail: usize,
    },
    Add {
        container: String,
        label: Option<String>,
        #[arg(short = 'p', long = "port")]
        port: Option<u16>,
        #[arg(short = 'n', long = "network")]
        network: Option<String>,
    },
    Remove {
        identifier: String,
    },
    Switch {
        identifier: String,
        port: Option<u16>,
    },
    Detect {
        filter: Option<String>,
    },
    Install,
    Tui,
}

fn print_lines(lines: Vec<String>) {
    for l in lines {
        println!("{l}");
    }
}

fn default_user_bin() -> Result<PathBuf> {
    let base = directories::BaseDirs::new()
        .ok_or_else(|| anyhow::anyhow!("Could not determine user directories"))?;
    Ok(base.home_dir().join(".local").join("bin"))
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let store = Store::new_default()?;

    match cli.command {
        Commands::Config => {
            let cfg = store.load()?;
            print_lines(vec![
                format!("Config file: {}", store.config_file.display()),
                "".to_string(),
                serde_json::to_string_pretty(&cfg)?,
            ]);
        }
        Commands::Install => {
            let exe = std::env::current_exe()?;
            let user_bin = default_user_bin()?;
            let out = install::install_hardlink(&exe, &user_bin)?;
            print_lines(vec![
                format!(
                    "Created hardlink: {} -> {}",
                    out.link_path.display(),
                    out.target_path.display()
                ),
                "".to_string(),
                "See 'proxy-manager --help' for a quick start guide.".to_string(),
            ]);
            if let Some(note) = out.path_notice {
                println!("{note}");
            }
        }
        Commands::List => {
            // List does not need Docker.
            let app = App::new(store, proxy_manager::docker::NoopDocker);
            print_lines(app.list_configured_containers()?);
        }
        Commands::Add {
            container,
            label,
            port,
            network,
        } => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.add_container(container, label, port, network).await?);
        }
        Commands::Remove { identifier } => {
            // Remove is purely config mutation.
            let app = App::new(store, proxy_manager::docker::NoopDocker);
            print_lines(app.remove_container(identifier).await?);
        }
        Commands::Switch { identifier, port } => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.switch_target(identifier, port).await?);
        }
        Commands::Detect { filter } => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.detect_containers(filter).await?);
        }
        Commands::Networks => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.list_networks().await?);
        }
        Commands::Start => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.start_proxy().await?);
        }
        Commands::Stop { port } => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            if let Some(p) = port {
                print_lines(app.stop_port(p).await?);
            } else {
                print_lines(app.stop_proxy().await?);
            }
        }
        Commands::Restart => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.restart_proxy().await?);
        }
        Commands::Reload => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.reload_proxy().await?);
        }
        Commands::Status => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.status().await?);
        }
        Commands::Logs { follow, tail } => {
            let docker = BollardDocker::connect_local()?;
            let app = App::new(store, docker);
            print_lines(app.logs(follow, tail).await?);
        }
        Commands::Tui => {
            let docker = BollardDocker::connect_local()?;
            proxy_manager::tui::run(store, docker).await?;
        }
    }

    Ok(())
}
