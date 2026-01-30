mod config;
mod docker;
mod nginx;
mod ops;
mod paths;
mod storage;

fn main() {
    let config = config::AppConfig::default();
    let nginx = nginx::generate_nginx_config(&config);
    let config_path = paths::config_file();
    println!(
        "proxy-manager (rust) - scaffold in progress\nconfig: {}\nnginx bytes: {}",
        config_path.display(),
        nginx.len()
    );
}
