mod cli;
mod config;
mod docker;
mod nginx;
mod tui;

use anyhow::Result;
use clap::Parser;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing subscriber for logging
    tracing_subscriber::fmt::init();

    // Check if running in TUI mode (no arguments or "tui" command)
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "tui" {
        // Run TUI
        let app = tui::TuiApp::new().await?;
        app.run().await?;
    } else {
        // Run CLI
        let cli = cli::Cli::parse();
        let handler = cli::CliHandler::new().await?;
        handler.run(cli).await?;
    }

    Ok(())
}
