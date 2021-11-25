use std::time::Duration;

use async_std::{
    channel::{self, Sender},
    future::pending,
    prelude::FutureExt as AsyncStdFutureExt,
};
use url::Url;

use crate::{
    cmd_utils::{get_folder_size, Duplicity, Rclone},
    config::BackupConfig,
    rcon::MinecraftCommand,
};

pub struct BackupManager;

impl BackupManager {
    pub async fn start(
        config: Option<BackupConfig>,
        cmd_chan: Sender<MinecraftCommand>,
    ) -> Vec<String> {
        if let Some(config) = config {
            let (back_send, back_rec) = channel::bounded(1);

            let world_folder = match config.world_folder.into_os_string().into_string() {
                Ok(p) => p,
                Err(_) => {
                    return vec!["[BACKUP] Failed to convert world path to string.".into()];
                }
            };

            let backup_folder = match config.backup_folder.into_os_string().into_string() {
                Ok(p) => p,
                Err(_) => {
                    return vec!["[BACKUP] Failed to convert backup path to string.".into()];
                }
            };

            let backup_folder_url = match Url::from_file_path(&backup_folder) {
                Ok(p) => p,
                Err(_) => {
                    return vec!["[BACKUP] Failed to make path of world folder.".into()];
                }
            };

            let mut waiter = async_std::task::sleep(config.incremental);
            loop {
                waiter.await;
                waiter = async_std::task::sleep(config.incremental);

                println!("[ServerManager] [BACKUP] Sarting backup...");

                if !config.silent {
                    match cmd_chan
                        .send(MinecraftCommand::Broadcast("Backup started.".into()))
                        .timeout(Duration::from_secs(10))
                        .await
                    {
                        Err(_) => {
                            return vec![
                                "[BACKUP] Timed out while broadcasting start message.".into()
                            ];
                        }
                        Ok(Err(_)) => {
                            return vec!["[BACKUP] Failed to broadcast start message.".into()];
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
                        return vec![
                            "[BACKUP] Timed out while requesting to disable saving.".into()
                        ];
                    }
                    Ok(Err(_)) => {
                        return vec!["[BACKUP] Failed to disable saving.".into()];
                    }
                    _ => (),
                }

                match cmd_chan
                    .send(MinecraftCommand::SaveAll(config.flush_on_save))
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        return vec!["[BACKUP] Timed out while requesting save.".into()];
                    }
                    Ok(Err(_)) => {
                        return vec!["[BACKUP] Failed to save.".into()];
                    }
                    _ => (),
                }

                match cmd_chan
                    .send(MinecraftCommand::Await(back_send.clone()))
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        return vec![
                            "[BACKUP] Timed out while requesting to send await handle.".into()
                        ];
                    }
                    Ok(Err(_)) => {
                        return vec!["[BACKUP] Failed to send await handle.".into()];
                    }
                    _ => (),
                }

                match back_rec.recv().timeout(Duration::from_secs(2 * 60)).await {
                    Err(_) => {
                        return vec!["[BACKUP] Timed out while waiting for backup.".into()];
                    }
                    Ok(Err(_)) => {
                        return vec!["[BACKUP] Failed to wait for save completion.".into()];
                    }
                    _ => (),
                }

                if !config.flush_on_save {
                    async_std::task::sleep(Duration::from_secs(2 * 60)).await;
                }

                if let Err(x) = Duplicity::backup(
                    config.full_backup_every,
                    &world_folder,
                    backup_folder_url.as_str(),
                )
                .await
                {
                    return vec![format!(
                        "[BACKUP] Failed to perform duplicity backup:\n{}",
                        x
                    )];
                }

                match cmd_chan
                    .send(MinecraftCommand::SaveOn)
                    .timeout(Duration::from_secs(10))
                    .await
                {
                    Err(_) => {
                        return vec![
                            "[BACKUP] Timed out while requesting to disable saving.".into()
                        ];
                    }
                    Ok(Err(_)) => {
                        return vec!["[BACKUP] Failed to disable saving.".into()];
                    }
                    _ => (),
                }

                println!("[ServerManager] [BACKUP] Backup complete.");

                if !config.silent {
                    let backup_msg = if let Ok(folder_size) = get_folder_size(&world_folder).await {
                        format!(
                            "Backup done! ({:.2} GB)",
                            folder_size as f64 / (1024u64.pow(3) as f64)
                        )
                    } else {
                        "Backup done! (failed to get size)".into()
                    };

                    match cmd_chan
                        .send(MinecraftCommand::Broadcast(backup_msg))
                        .timeout(Duration::from_secs(10))
                        .await
                    {
                        Err(_) => {
                            return vec![
                                "[BACKUP] Timed out while broadcasting start message.".into()
                            ];
                        }
                        Ok(Err(_)) => {
                            return vec!["[BACKUP] Failed to broadcast start message.".into()];
                        }
                        _ => (),
                    }
                }

                if let Err(x) =
                    Duplicity::cleanup_old(config.keep_full_backup, backup_folder_url.as_str())
                        .await
                {
                    return vec![format!(
                        "[BACKUP] Failed to perform duplicity cleanup:\n{}",
                        x
                    )];
                }

                if let Some(remote) = &config.rclone_path {
                    let mut sync_attempts = 0u32;

                    let mut err = None;
                    while sync_attempts < 5 {
                        if let Err(new_err) = Rclone::sync(remote, &backup_folder).await {
                            sync_attempts += 1;
                            err = Some(new_err);
                        } else {
                            break;
                        }
                    }

                    if let Some(err) = err {
                        if sync_attempts >= 5 {
                            return vec![format!("[ServerManager] [BACKUP] Failed to sync backup data to remote:\n{}", err)];
                        } else {
                            println!("[ServerManager] [BACKUP] At least one recoverable error occured while trying to sync backup data to remote:\n{}", err);
                        }
                    }

                    println!("[ServerManager] [BACKUP] Remote backup sync complete.")
                }
            }
        } else {
            pending::<()>().await;
            unreachable!()
        }
    }
}
