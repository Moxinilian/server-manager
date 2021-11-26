use std::{
    ops::{Deref, DerefMut},
    process::Stdio,
    time::{Duration, Instant},
};

use crate::{
    backup::BackupManager,
    config::Config,
    mail::{MailManager, MailRequest},
    rcon::RconManager,
};

use anyhow::Result;
use async_std::process::{Child, Command};
use async_std::{
    channel::{self},
    prelude::FutureExt as AsyncStdFutureExt,
};
use chrono::Utc;
use futures::{pin_mut, select, FutureExt};
use nix::sys::signal::{self, Signal};

pub struct ChildKiller(pub Child);

impl Drop for ChildKiller {
    fn drop(&mut self) {
        async_std::task::block_on(ServerManager::emergency_shutdown(&mut self.0));
    }
}

impl Deref for ChildKiller {
    type Target = Child;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl DerefMut for ChildKiller {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

pub struct ServerManager;

impl ServerManager {
    pub async fn start(config: Config) -> Result<()> {
        let mut last_incident = Instant::now();
        let mut recent_incidents = 0;

        let mail_handles = if let Some(mail_config) = &config.mailing {
            MailManager::test_mail(mail_config.clone(), &config.name)
                .await
                .map_err(|e| {
                    println!("[ServerManager] [MAIL] Failed to send startup email.");
                    e
                })?;

            let (snd, rec) = channel::bounded(32);

            let mail_task = async_std::task::spawn(MailManager::start(
                mail_config.clone(),
                config.name.clone(),
                rec,
            ));

            Some((mail_task, snd))
        } else {
            None
        };

        loop {
            let serv_handle = Command::new(&config.java)
                .args(&config.java_args)
                .arg("-jar")
                .arg(&config.server_jar)
                .arg("--nogui")
                .stdout(Stdio::inherit())
                .stderr(Stdio::inherit())
                .stdin(Stdio::inherit())
                .current_dir(&config.server_folder)
                .spawn()?;

            let mut serv_handle = ChildKiller(serv_handle);

            let (cmd_send, cmd_rec) = channel::bounded(32);

            let rcon_man = RconManager::start(config.clone(), cmd_rec).fuse();
            let backup_man = BackupManager::start(config.backups.clone(), cmd_send).fuse();
            let serv_man = serv_handle.status().fuse();

            pin_mut!(rcon_man, backup_man, serv_man);

            let err_log = select! {
                res = serv_man => {
                    let mut err_log = vec!["Spontaneous server exit.".into()];
                    match res {
                        Err(err) => {
                            err_log.push(
                                format!("An error occured while obtaining server exit status:\n{}",
                                err
                            ));
                        }
                        Ok(status) => {
                            err_log.push(format!("Status code: {}", status));
                        }
                    }

                    err_log
                }
                mut err_log = rcon_man => {
                    Self::emergency_shutdown(&mut serv_handle).await;
                    err_log.push("Emergency server shutdown caused by RCON failure.".into());
                    err_log
                }
                mut err_log = backup_man => {
                    Self::emergency_shutdown(&mut serv_handle).await;
                    err_log.push("Emergency server shutdown caused by backup failure.".into());
                    err_log
                }
            };

            for e in &err_log {
                println!("[ServerManager] {}", e);
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

                    if let Some((_, chan)) = &mail_handles {
                        chan.send(MailRequest {
                            err_log,
                            final_incident: true,
                            time: Utc::now(),
                        })
                        .await
                        .ok();
                    }

                    break;
                } else {
                    if let Some((_, chan)) = &mail_handles {
                        chan.send(MailRequest {
                            err_log,
                            final_incident: false,
                            time: Utc::now(),
                        })
                        .await
                        .ok();
                    }

                    last_incident = Instant::now();
                    println!("[ServerManager] Restarting in 10 seconds...");
                    async_std::task::sleep(Duration::from_secs(10)).await;
                }
            } else {
                println!("[ServerManager] Auto-restart is disabled. Exiting.");
                if let Some((_, chan)) = &mail_handles {
                    chan.send(MailRequest {
                        err_log,
                        final_incident: true,
                        time: Utc::now(),
                    })
                    .await
                    .ok();
                }
                break;
            }
        }

        if let Some((handle, _)) = mail_handles {
            handle.await?;
        }

        Ok(())
    }

    pub async fn emergency_shutdown(serv_handle: &mut Child) {
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
