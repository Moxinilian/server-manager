use std::path::PathBuf;

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

    let config_file = if let Some(config_path) = std::env::args().nth(1) {
        PathBuf::from(config_path)
    } else {
        PathBuf::from(".").join("server-manager.ron")
    };
    let config = if config_file.exists() {
        Config::try_from(config_file.as_ref()).await.map_err(|e| {
            println!("[ServerManager] The provided file is not a valid configuration file.");
            e
        })?
    } else {
        if std::env::args().len() > 1 {
            println!("[ServerManager] The provided file does not exist.");
        } else {
            ConfigSerialized::default().save(&config_file)?;
            println!("[ServerManager] No manager configuration found.");
            println!("[ServerManager] Generated a dummy configuration file.");
        }

        return Ok(());
    };

    println!("[ServerManager] Starting server...");

    ServerManager::start(config).await?;

    Ok(())
}
