use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{anyhow, Result};
use lettre::{
    message::{Mailbox, Mailboxes},
    transport::smtp::authentication::Credentials,
};
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::cmd_utils::{Duplicity, Rclone};

#[derive(Serialize, Deserialize)]
pub struct ConfigSerialized {
    name: String,
    auto_restart: bool,
    server_folder: String,
    server_jar: String,
    backups: Option<BackupConfigSerialized>,
    java: String,
    java_args: Vec<String>,
    rcon_password: String,
    rcon_port: u16,
    mailing: Option<MailConfigSerialized>,
}

#[derive(Serialize, Deserialize)]
pub struct BackupConfigSerialized {
    backup_folder: String,
    world_folder: String,
    incremental_freq_hours: u64,
    full_backup_every: u32,
    keep_full_backup: u32,
    rclone_path: Option<String>,
    flush_on_save: bool,
    silent: bool,
}

#[derive(Serialize, Deserialize)]
pub struct MailConfigSerialized {
    contacts: Vec<String>,
    smtp_server: String,
    sender: String,
    username: String,
    password: String,
}

impl ConfigSerialized {
    pub fn save(&self, path: &Path) -> Result<()> {
        let file = std::fs::File::create(path)?;
        ron::ser::to_writer_pretty(file, self, Default::default())?;
        Ok(())
    }
}

impl Default for ConfigSerialized {
    fn default() -> Self {
        let rcon_key: [u8; 48] = rand::thread_rng().gen();

        Self {
            name: "Minecraft Server".into(),
            auto_restart: true,
            server_folder: "./".into(),
            server_jar: "minecraft_server.jar".into(),
            java: "java".into(),
            java_args: Vec::new(),
            rcon_password: base64::encode(rcon_key),
            rcon_port: 25575,
            mailing: None,
            backups: Some(Default::default()),
        }
    }
}

impl Default for BackupConfigSerialized {
    fn default() -> Self {
        Self {
            backup_folder: "./backups".into(),
            world_folder: "world".into(),
            incremental_freq_hours: 1,
            full_backup_every: 24 * 14,
            keep_full_backup: 2,
            rclone_path: None,
            flush_on_save: true,
            silent: false,
        }
    }
}

#[derive(Clone)]
pub struct Config {
    pub name: String,
    pub auto_restart: bool,
    pub server_folder: PathBuf,
    pub server_jar: PathBuf,
    pub backups: Option<BackupConfig>,
    pub rcon_password: String,
    pub rcon_port: u16,
    pub java: String,
    pub java_args: Vec<String>,
    pub mailing: Option<MailConfig>,
}

impl Config {
    pub async fn try_from_serialized(value: ConfigSerialized) -> Result<Self> {
        let server_folder = std::fs::canonicalize(&value.server_folder)
            .map_err(|_| anyhow!("failed to find server folder at {}", value.server_folder))?;

        if !server_folder.is_dir() {
            return Err(anyhow!(
                "the node at {} is not a folder\nNote: the path above is equivalent to {:?}",
                value.server_folder,
                server_folder
            ));
        }

        let server_jar_relative = PathBuf::from(&value.server_folder).join(value.server_jar);
        let server_jar = std::fs::canonicalize(&server_jar_relative).map_err(|_| {
            anyhow!(
                "failed to find server jar at {:?}\nNote: the server folder is at {:?}",
                server_jar_relative,
                server_folder
            )
        })?;

        let backups = if let Some(backups) = value.backups {
            Some(BackupConfig::try_from_serialized(backups, &server_folder).await?)
        } else {
            None
        };

        let mailing = if let Some(mailing) = value.mailing {
            Some(MailConfig::try_from_serialized(mailing).await?)
        } else {
            None
        };

        Ok(Self {
            name: value.name,
            auto_restart: value.auto_restart,
            server_folder,
            server_jar,
            backups,
            rcon_password: value.rcon_password,
            rcon_port: value.rcon_port,
            java: value.java,
            java_args: value.java_args,
            mailing,
        })
    }

    pub async fn try_from(path: &Path) -> Result<Self> {
        let config_ser: ConfigSerialized = ron::de::from_reader(std::fs::File::open(path)?)?;
        Self::try_from_serialized(config_ser).await
    }
}

#[derive(Clone)]
pub struct BackupConfig {
    pub backup_folder: PathBuf,
    pub world_folder: PathBuf,
    pub incremental: Duration,
    pub full_backup_every: u32,
    pub keep_full_backup: u32,
    pub rclone_path: Option<String>,
    pub flush_on_save: bool,
    pub silent: bool,
}

impl BackupConfig {
    pub async fn try_from_serialized(
        config: BackupConfigSerialized,
        server_folder: &Path,
    ) -> Result<Self> {
        if !Duplicity::is_available().await? {
            return Err(anyhow!(
                "duplicity is not available but config requests its use"
            ));
        }

        let mut backup_folder = std::fs::canonicalize(&config.backup_folder)
            .map_err(|_| anyhow!("failed to find backup folder at {}", config.backup_folder))?;

        if backup_folder.ends_with("/") {
            backup_folder.pop();
        }

        if !backup_folder.is_dir() {
            return Err(anyhow!(
                "backup folder `{:?}` is not a folder",
                backup_folder
            ));
        }

        let mut world_folder = server_folder.join(config.world_folder);
        if world_folder.ends_with("/") {
            world_folder.pop();
        }

        if config.incremental_freq_hours == 0 {
            return Err(anyhow!("incremental backup frequency must not be zero"));
        }

        if let Some(path) = &config.rclone_path {
            if !Rclone::is_available().await? {
                return Err(anyhow!(
                    "rclone is not available but config requests its use"
                ));
            }

            Rclone::check_path(path).await?;
        }

        Ok(Self {
            backup_folder,
            world_folder,
            incremental: Duration::from_secs(config.incremental_freq_hours * 60 * 60),
            full_backup_every: config.full_backup_every,
            keep_full_backup: config.keep_full_backup,
            rclone_path: config.rclone_path,
            flush_on_save: config.flush_on_save,
            silent: config.silent,
        })
    }
}

#[derive(Clone)]
pub struct MailConfig {
    pub smtp_server: String,
    pub contacts: Mailboxes,
    pub sender: Mailbox,
    pub credentials: Credentials,
}

impl MailConfig {
    pub async fn try_from_serialized(config: MailConfigSerialized) -> Result<Self> {
        let sender = config.sender.parse()?;

        let mut contacts = Vec::with_capacity(config.contacts.len());

        for c in config.contacts {
            contacts.push(c.parse()?);
        }

        let credentials = Credentials::new(config.username, config.password);

        Ok(Self {
            smtp_server: config.smtp_server,
            sender,
            contacts: contacts.into(),
            credentials,
        })
    }
}
