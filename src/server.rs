use std::{
    process::Stdio,
    time::{Duration, Instant},
};

use crate::{backup_utils::{Duplicity, get_folder_size}, config::{BackupConfig, Config}};

use anyhow::{anyhow, Result};
use async_std::process::{Child, Command};
use async_std::{
    channel::{self, Receiver, Sender},
    future::pending,
    prelude::FutureExt as AsyncStdFutureExt,
    task::spawn,
};
use futures::{
    future::{select, AbortHandle, Abortable, Fuse},
    pin_mut, select, FutureExt,
};
use nix::sys::signal::{self, Signal};

pub enum MinecraftCommand {
    SaveOn,
    SaveAll(bool),
    SaveOff,
    Broadcast(String),
    Await(Sender<()>),
}

pub struct ServerManager;

impl ServerManager {
    pub async fn start(config: Config) -> Result<()> {
        let mut last_incident = Instant::now();
        let mut recent_incidents = 0;

        loop {
            let mut serv_handle = Command::new(&config.java)
                .args(&config.java_args)
                .arg("-jar")
                .arg(&config.server_jar)
                .arg("--nogui")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .stdin(Stdio::inherit())
                .spawn()?;

            let (cmd_send, cmd_rec) = channel::bounded(32);

            let rcon_man = RconManager::start(config.clone(), cmd_rec).fuse();
            let backup_man = BackupManager::start(config.backups.clone(), cmd_send).fuse();
            let serv_man = serv_handle.status().fuse();

            pin_mut!(rcon_man, backup_man, serv_man);

            select! {
                res = serv_man => {
                    if let Err(err) = res {
                        println!(
                            "[ServerManager] An error occured while obtaining server exit status:\n{}",
                            err
                        );
                    }
                }
                _ = rcon_man => {
                    println!("[ServerManager] Emergency server shutdown caused by RCON failure.");
                    Self::emergency_shutdown(&mut serv_handle).await;
                }
                _ = backup_man => {
                    println!("[ServerManager] Emergency server shutdown caused by backup failure.");
                    Self::emergency_shutdown(&mut serv_handle).await;
                }
            }

            println!("[ServerManager] The server exited.");
            if config.auto_restart {
                if (Instant::now() - last_incident) > Duration::from_secs(15 * 60) {
                    recent_incidents = 0;
                }

                recent_incidents += 1;

                if recent_incidents > 5 {
                    println!(
                        "[ServerManager] Too many incidents in a short period of time. Exiting."
                    );
                    break;
                } else {
                    last_incident = Instant::now();
                    println!("[ServerManager] Restarting in 10 seconds...");
                    async_std::task::sleep(Duration::from_secs(10)).await;
                }
            } else {
                println!("[ServerManager] Auto-restart is disabled. Exiting.");
                break;
            }
        }

        Ok(())
    }

    async fn emergency_shutdown(serv_handle: &mut Child) {
        let pid = nix::unistd::Pid::from_raw(serv_handle.id() as i32);

        signal::kill(pid, Signal::SIGINT).ok();
        if serv_handle
            .status()
            .timeout(Duration::from_secs(20))
            .await
            .is_err()
        {
            signal::kill(pid, Signal::SIGKILL).ok();
            serv_handle.status().await.ok();
        }
    }
}

struct RconManager;

impl RconManager {
    pub async fn start(config: Config, chan: Receiver<MinecraftCommand>) {
        let mut last_incident = Instant::now();
        let mut recent_incidents = 0;
        let mut first_attempt = true;

        let mut first_attempt_attempts = 0;

        loop {
            if let Err(err) = Self::inner(&config, &chan, first_attempt).await {
                println!("[ServerManager] [RCON] Unexpected failure.\n{}", err);

                first_attempt = false;

                if (Instant::now() - last_incident) > Duration::from_secs(10 * 60) {
                    recent_incidents = 0;
                }

                if recent_incidents > 5 {
                    println!(
                        "[ServerManager] [RCON] Too many RCON incidents in a short period of time."
                    );
                    break;
                } else {
                    last_incident = Instant::now();
                    println!("[ServerManager] [RCON] Reconnecting...");
                    async_std::task::sleep(Duration::from_secs(1)).await;
                }
            } else {
                first_attempt_attempts += 1;

                if first_attempt_attempts > 60 {
                    println!("[ServerManager] [RCON] Server took too long before first contact.");
                    break;
                }

                async_std::task::sleep(Duration::from_secs(10)).await;
            }
        }
    }

    async fn inner(
        config: &Config,
        chan: &Receiver<MinecraftCommand>,
        first_attempt: bool,
    ) -> Result<()> {
        let address = String::from("localhost:") + &config.rcon_port.to_string();

        let mut conn = match rcon::Connection::builder()
            .enable_minecraft_quirks(true)
            .connect(address, &config.rcon_password)
            .await
        {
            Ok(conn) => conn,
            Err(err) => match err {
                rcon::Error::Io(err) => {
                    return if first_attempt {
                        Ok(())
                    } else {
                        Err(err.into())
                    }
                }
                x => return Err(x.into()),
            },
        };

        loop {
            match chan.recv().await? {
                MinecraftCommand::SaveOn => drop(dbg!(conn.cmd("save-on").await?)),
                MinecraftCommand::SaveAll(flush) => {
                    dbg!(
                        conn.cmd(if flush { "save-all flush" } else { "save-all" })
                            .await?
                    );
                }
                MinecraftCommand::SaveOff => drop(dbg!(conn.cmd("save-off").await?)),
                MinecraftCommand::Broadcast(msg) => {
                    conn.cmd(&format!(
                        "tellraw @a {{\"text\":\"{}\",\"color\":\"light_purple\"}}",
                        msg
                    ))
                    .await?;
                }
                MinecraftCommand::Await(back) => back.send(()).await?,
            }
        }
    }
}

struct BackupManager;

impl BackupManager {
    pub async fn start(config: Option<BackupConfig>, cmd_chan: Sender<MinecraftCommand>) {
        if let Some(config) = config {
            let (back_send, back_rec) = channel::bounded(0);

            let world_folder = match config.world_folder.into_os_string().into_string() {
                Ok(p) => p,
                Err(e) => {
                    println!("[ServerManager] [BACKUP] Failed to convert world path to string.");
                    return;
                }
            };

            let backup_folder = match config.backup_folder.into_os_string().into_string() {
                Ok(p) => p,
                Err(e) => {
                    println!("[ServerManager] [BACKUP] Failed to convert backup path to string.");
                    return;
                }
            };

            loop {
                async_std::task::sleep(config.incremental).await;

                if !config.silent {
                    match cmd_chan
                        .send(MinecraftCommand::Broadcast("Backup started.".into()))
                        .timeout(Duration::from_secs(10))
                        .await
                    {
                        Err(_) => {
                            println!("[ServerManager] [BACKUP] Timed out while broadcasting start message.");
                            return;
                        }
                        Ok(Err(_)) => {
                            println!("[ServerManager] [BACKUP] Failed to broadcast start message.");
                            return;
                        }
                        _ => (),
                    }
                }

                match cmd_chan
                    .send(MinecraftCommand::SaveOff)
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        println!("[ServerManager] [BACKUP] Timed out while requesting to disable saving.");
                        return;
                    }
                    Ok(Err(_)) => {
                        println!("[ServerManager] [BACKUP] Failed to disable saving.");
                        return;
                    }
                    _ => (),
                }

                match cmd_chan
                    .send(MinecraftCommand::SaveAll(config.flush_on_save))
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        println!("[ServerManager] [BACKUP] Timed out while requesting save.");
                        return;
                    }
                    Ok(Err(_)) => {
                        println!("[ServerManager] [BACKUP] Failed to save.");
                        return;
                    }
                    _ => (),
                }

                match cmd_chan
                    .send(MinecraftCommand::Await(back_send.clone()))
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        println!("[ServerManager] [BACKUP] Timed out while requesting to send await handle.");
                        return;
                    }
                    Ok(Err(_)) => {
                        println!("[ServerManager] [BACKUP] Failed to send await handle.");
                        return;
                    }
                    _ => (),
                }

                match back_rec.recv().timeout(Duration::from_secs(2 * 60)).await {
                    Err(_) => {
                        println!("[ServerManager] [BACKUP] Timed out while waiting for backup.");
                        return;
                    }
                    Ok(Err(_)) => {
                        println!("[ServerManager] [BACKUP] Failed to wait for save completion.");
                        return;
                    }
                    _ => (),
                }

                if !config.flush_on_save {
                    async_std::task::sleep(Duration::from_secs(2 * 60)).await;
                }

                if let Err(x) =
                    Duplicity::backup(config.full_backup_every, &world_folder, &backup_folder).await
                {
                    println!(
                        "[ServerManager] [BACKUP] Failed to perform duplicity backup:\n{}",
                        x
                    );
                    return;
                }

                match cmd_chan
                    .send(MinecraftCommand::SaveOn)
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        println!("[ServerManager] [BACKUP] Timed out while requesting to disable saving.");
                        return;
                    }
                    Ok(Err(_)) => {
                        println!("[ServerManager] [BACKUP] Failed to disable saving.");
                        return;
                    }
                    _ => (),
                }

                if !config.silent {
                    let backup_msg = if let Ok(folder_size) = get_folder_size(&world_folder).await {
                        format!("Backup done! ({:.2} GB)", folder_size as f64 / (1024u64.pow(3) as f64))
                    } else {
                        "Backup done! (failed to get size)".into()
                    };

                    match cmd_chan
                        .send(MinecraftCommand::Broadcast(backup_msg))
                        .timeout(Duration::from_secs(10))
                        .await
                    {
                        Err(_) => {
                            println!("[ServerManager] [BACKUP] Timed out while broadcasting start message.");
                            return;
                        }
                        Ok(Err(_)) => {
                            println!("[ServerManager] [BACKUP] Failed to broadcast start message.");
                            return;
                        }
                        _ => (),
                    }
                }

                if let Err(x) =
                    Duplicity::cleanup_old(config.keep_full_backup, &backup_folder).await
                {
                    println!(
                        "[ServerManager] [BACKUP] Failed to perform duplicity cleanup:\n{}",
                        x
                    );
                    return;
                }

                let mut sync_attempts = 0;
                todo!();
            }
        } else {
            pending::<()>().await;
        }
    }
}
