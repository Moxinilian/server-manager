use std::time::{Duration, Instant};

use anyhow::Result;
use async_std::channel::{Receiver, Sender};
use rcon::Connection;

use crate::config::Config;

pub enum MinecraftCommand {
    SaveOn,
    SaveAll(bool),
    SaveOff,
    Broadcast(String),
    Await(Sender<()>),
}

pub struct RconError {
    error: anyhow::Error,
    cmd: Option<MinecraftCommand>,
}

impl<T> From<T> for RconError where T: std::error::Error + Send + Sync + 'static {
    fn from(err: T) -> Self {
        Self {
            error: err.into(),
            cmd: None,
        }
    }
}

pub struct RconManager;

impl RconManager {
    pub async fn start(config: Config, chan: Receiver<MinecraftCommand>) -> Vec<String> {
        let mut last_incident = Instant::now();
        let mut recent_incidents = 0;
        let mut first_attempt = true;

        let mut first_attempt_attempts = 0;

        let mut pending_msg = None;

        let err_log = loop {
            if let Err(err) = Self::inner(&config, &chan, first_attempt, pending_msg).await {
                println!("[ServerManager] [RCON] Unexpected failure.\n{}", err.error);

                first_attempt = false;

                if (Instant::now() - last_incident) > Duration::from_secs(10 * 60) {
                    recent_incidents = 0;
                }

                if recent_incidents > 5 {
                    break vec!["[RCON] Too many RCON incidents in a short period of time.".into()];
                } else {
                    last_incident = Instant::now();
                    println!("[ServerManager] [RCON] Reconnecting...");
                    async_std::task::sleep(Duration::from_secs(1)).await;
                }

                pending_msg = err.cmd;
            } else {
                first_attempt_attempts += 1;

                if first_attempt_attempts > 60 {
                    break vec!["[RCON] Server took too long before first contact.".into()];
                }

                async_std::task::sleep(Duration::from_secs(10)).await;

                pending_msg = None;
            }
        };

        err_log
    }

    async fn inner(
        config: &Config,
        chan: &Receiver<MinecraftCommand>,
        first_attempt: bool,
        pending_message: Option<MinecraftCommand>,
    ) -> Result<(), RconError> {
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

        println!("[ServerManager] [RCON] Acquired connection to server.");

        if let Some(pending) = pending_message {
            if let Err(error) = Self::send_message(&mut conn, &pending).await {
                return Err(RconError {
                    error: error.into(),
                    cmd: Some(pending),
                });
            }

            println!("[ServerManager] [RCON] Pending message processed.");
        }

        loop {
            let cmd = chan.recv().await?;
            if let Err(error) = Self::send_message(&mut conn, &cmd).await {
                return Err(RconError {
                    error: error.into(),
                    cmd: Some(cmd),
                });
            }
        }
    }

    async fn send_message(conn: &mut Connection, cmd: &MinecraftCommand) -> Result<(), rcon::Error> {
        match &cmd {
            MinecraftCommand::SaveOn => conn.cmd("save-on").await.map(drop),
            MinecraftCommand::SaveAll(flush) => conn
                .cmd(if *flush { "save-all flush" } else { "save-all" })
                .await
                .map(drop),
            MinecraftCommand::SaveOff => conn.cmd("save-off").await.map(drop),
            MinecraftCommand::Broadcast(msg) => conn
                .cmd(&format!(
                    "tellraw @a {{\"text\":\"{}\",\"color\":\"light_purple\"}}",
                    msg
                ))
                .await
                .map(drop),
            MinecraftCommand::Await(back) => {
                back.send(()).await.ok();
                Ok(())
            }
        }
    }
}
