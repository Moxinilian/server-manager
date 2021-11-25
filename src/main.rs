use anyhow::Result;

use crate::{
    config::{Config, ConfigSerialized},
    server::ServerManager,
};

mod backup;
mod cmd_utils;
mod config;
mod mail;
mod rcon;
mod server;

#[async_std::main]
async fn main() -> Result<()> {
    println!("[ServerManager] Fetching config...");

    let config_file = std::path::PathBuf::from(".").join("server-manager.ron");
    let config = if config_file.exists() {
        Config::try_from(config_file.as_ref()).await?
    } else {
        ConfigSerialized::default().save(&config_file)?;
        println!("[ServerManager] No manager configuration found.");
        println!("[ServerManager] Generated a dummy configuration file.");
        return Ok(());
    };

    println!("[ServerManager] Starting server...");

    ServerManager::start(config).await?;

    Ok(())
}
