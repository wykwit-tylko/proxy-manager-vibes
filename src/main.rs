mod cli;
mod config;
mod docker;
mod nginx;
mod proxy;
mod tui;
mod utils;

use cli::handler;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    handler::run().await
}
