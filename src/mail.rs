use std::time::Duration;

use anyhow::Result;
use async_std::channel::{Receiver, TryRecvError};
use chrono::{DateTime, Utc};
use lettre::{
    message::header::{ContentType, To},
    AsyncSmtpTransport, AsyncStd1Executor, AsyncTransport, Message,
};

use crate::config::MailConfig;

pub struct MailRequest {
    pub err_log: Vec<String>,
    pub final_incident: bool,
    pub time: DateTime<Utc>,
}

pub struct MailManager;

impl MailManager {
    pub async fn test_mail(config: MailConfig, name: &str) -> Result<()> {
        let email = Message::builder()
            .from(config.sender)
            .mailbox::<To>(config.contacts.into())
            .header(ContentType::TEXT_HTML)
            .subject(format!("{} - Minecraft Server Manager Started", name))
            .body(format!(
                "On {}, the Minecraft server manager for \"{}\" started.",
                Utc::now(),
                name
            ))?;

        AsyncSmtpTransport::<AsyncStd1Executor>::relay(&config.smtp_server)?
            .credentials(config.credentials)
            .build()
            .send(email)
            .await
            .map(drop)
            .map_err(Into::into)
    }

    pub async fn start(
        config: MailConfig,
        name: String,
        mail_rec: Receiver<MailRequest>,
    ) -> Result<()> {
        let mut mail_requests = Vec::new();
        loop {
            mail_requests.clear();
            mail_requests.push(mail_rec.recv().await?);

            loop {
                async_std::task::sleep(Duration::from_secs(30)).await;

                match mail_rec.try_recv() {
                    Ok(mail) => mail_requests.push(mail),
                    Err(TryRecvError::Empty) => break,
                    Err(x) => return Err(x.into()),
                }

                loop {
                    match mail_rec.try_recv() {
                        Ok(mail) => mail_requests.push(mail),
                        Err(TryRecvError::Empty) => break,
                        Err(x) => return Err(x.into()),
                    }
                }
            }

            let is_final = mail_requests.iter().any(|x| x.final_incident);

            let subject = if is_final {
                format!("URGENT - {} - Server Manager stopped after incident", name)
            } else {
                format!("{} - Incident report", name)
            };

            let mut body = format!(
                "On {}, the Minecraft server \"{}\" encountered an incident.<br><br>&emsp;Error report:<br>{}<br><br>",
                mail_requests[0].time,
                name,
                mail_requests[0].err_log.iter().fold(String::from("&emsp;"), |acc, x| (acc + "<br>&emsp;") + x),
            );

            for x in mail_requests.iter().skip(1) {
                body += &format!(
                    "Additionally, on {}, another incident occured.<br><br>&emsp;Error report:<br>{}<br><br>",
                    x.time,
                    x.err_log
                        .iter()
                        .fold(String::from("&emsp;"), |acc, x| (acc + "<br>&emsp;") + x),
                );
            }

            if is_final {
                body += "<b>After this incident, the server manager stopped.</b><br>";
            }

            body += "End of report.";

            let email = Message::builder()
                .from(config.sender.clone())
                .mailbox::<To>(config.contacts.clone().into())
                .header(ContentType::TEXT_HTML)
                .subject(subject)
                .body(body)?;

            let mut attempts = 0;
            while let Err(err) = AsyncSmtpTransport::<AsyncStd1Executor>::relay("smtp.gmail.com")?
                .credentials(config.credentials.clone())
                .build()
                .send(email.clone())
                .await
            {
                attempts += 1;
                if attempts > 5 {
                    println!(
                        "[ServerManager] [MAIL] Failed to send incident report:\n{}",
                        err
                    );
                    break;
                }
            }

            if is_final {
                break;
            }
        }

        Ok(())
    }
}
