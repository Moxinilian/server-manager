use std::time::Duration;

use anyhow::Result;
use async_std::channel::{Receiver, TryRecvError};
use chrono::{DateTime, Utc};

use crate::config::MailConfig;

pub struct MailRequest {
    err_log: Vec<String>,
    final_incident: bool,
    time: DateTime<Utc>,
}

pub struct MailManager;

impl MailManager {
    pub async fn start(config: MailConfig, mail_rec: Receiver<MailRequest>) -> Result<()> {
        loop {
            let mut mail_requests = vec![mail_rec.recv().await?];

            loop {
                async_std::task::sleep(Duration::from_secs(10)).await;
                match mail_rec.try_recv() {
                    Ok(mail) => mail_requests.push(mail),
                    Err(TryRecvError::Empty) => break,
                    Err(x) => return Err(x.into()),
                }
            }

            // send the mail
            todo!()
        }
    }
}
