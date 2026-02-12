use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    proxy_manager::cli::run_cli().await
}
