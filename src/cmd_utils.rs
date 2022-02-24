use std::{path::Path, process::Stdio};

use anyhow::{anyhow, Result};
use async_std::{io::ReadExt, process::Command};
use async_walkdir::WalkDir;
use futures::StreamExt;

pub struct Rclone;

impl Rclone {
    pub async fn is_available() -> Result<bool> {
        let mut child = Command::new("rclone")
            .arg("--help")
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        Ok(child.status().await?.success())
    }

    pub async fn check_path(path: &str) -> Result<()> {
        let mut child = Command::new("rclone")
            .arg("ls")
            .arg(path)
            .stderr(Stdio::piped())
            .spawn()?;

        if child.status().await?.success() {
            Ok(())
        } else {
            let err = if let Some(mut stderr) = child.stderr {
                let mut out = String::new();
                if stderr.read_to_string(&mut out).await.is_ok() {
                    out
                } else {
                    "failed to obtain error message (stderr failed)".into()
                }
            } else {
                "failed to obtain error message (no stderr)".into()
            };

            Err(anyhow!(
                "rclone failed to check for existing path:\n{}",
                err
            ))
        }
    }

    pub async fn sync(remote: &str, local: &str) -> Result<()> {
        // rclone sync local remote
        let mut child = Command::new("nice")
            .arg("-n")
            .arg("10")
            .arg("ionice")
            .arg("-c")
            .arg("3")
            .arg("rclone")
            .arg("sync")
            .arg(local)
            .arg(remote)
            .stderr(Stdio::piped())
            .spawn()?;

        if child.status().await?.success() {
            Ok(())
        } else {
            let err = if let Some(mut stderr) = child.stderr {
                let mut out = String::new();
                if stderr.read_to_string(&mut out).await.is_ok() {
                    out
                } else {
                    "failed to obtain error message (stderr failed)".into()
                }
            } else {
                "failed to obtain error message (no stderr)".into()
            };

            Err(anyhow!("rclone failed to sync to remote:\n{}", err))
        }
    }
}

pub struct Duplicity;

impl Duplicity {
    pub async fn is_available() -> Result<bool> {
        let mut child = Command::new("duplicity")
            .arg("--help")
            .stderr(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()?;

        Ok(child.status().await?.success())
    }

    pub async fn backup(
        full_if_older_than_hours: u32,
        to_backup: &str,
        backup_to: &str,
    ) -> Result<()> {
        let mut child = Command::new("nice")
            .arg("-n")
            .arg("10")
            .arg("ionice")
            .arg("-c")
            .arg("3")
            .arg("duplicity")
            .arg("--no-encryption")
            .arg("--allow-source-mismatch")
            .arg("--full-if-older-than")
            .arg(format!("{}h", full_if_older_than_hours))
            .arg(to_backup)
            .arg(backup_to)
            .stderr(Stdio::piped())
            .spawn()?;

        if child.status().await?.success() {
            Ok(())
        } else {
            let err = if let Some(mut stderr) = child.stderr {
                let mut out = String::new();
                if stderr.read_to_string(&mut out).await.is_ok() {
                    out
                } else {
                    "failed to obtain error message (stderr failed)".into()
                }
            } else {
                "failed to obtain error message (no stderr)".into()
            };

            Err(anyhow!("duplicity failed to make backup:\n{}", err))
        }
    }

    pub async fn cleanup_old(keep_full: u32, backup_to: &str) -> Result<()> {
        let mut child = Command::new("nice")
            .arg("-n")
            .arg("10")
            .arg("ionice")
            .arg("-c")
            .arg("3")
            .arg("duplicity")
            .arg("--allow-source-mismatch")
            .arg("remove-all-but-n-full")
            .arg(keep_full.to_string())
            .arg("--force")
            .arg(backup_to)
            .stderr(Stdio::piped())
            .spawn()?;

        if child.status().await?.success() {
            Ok(())
        } else {
            let err = if let Some(mut stderr) = child.stderr {
                let mut out = String::new();
                if stderr.read_to_string(&mut out).await.is_ok() {
                    out
                } else {
                    "failed to obtain error message (stderr failed)".into()
                }
            } else {
                "failed to obtain error message (no stderr)".into()
            };

            Err(anyhow!(
                "duplicity failed to clean up old backups:\n{}",
                err
            ))
        }
    }
}

pub async fn get_folder_size(path: impl AsRef<Path>) -> Result<u64> {
    let mut entries = WalkDir::new(path);
    let mut res = 0;
    loop {
        match entries.next().await {
            Some(Ok(entry)) => {
                res += entry.metadata().await?.len();
            }
            Some(Err(e)) => {
                return Err(e.into());
            }
            None => break,
        }
    }
    Ok(res)
}
