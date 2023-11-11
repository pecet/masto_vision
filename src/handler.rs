use std::error::Error;

use crate::{vision::Vision, config::Config};
use chrono::Local;

use futures_util::{TryFutureExt, TryStreamExt};
use kv_log_macro::warn;
use log::{LevelFilter, debug, info, error};
use mastodon_async::{Mastodon, prelude::*};

use clap::{Command, Arg, builder::PossibleValue};
pub struct Handler();
impl Handler {
    pub fn get_log_level(&self) -> LevelFilter {
        let matches = clap::Command::new("MastoVision")
            .version("0.1.0")
            .author("pecet")
            .about("Generates image descriptions for Mastodon")
            .arg(
                Arg::new("verbosity level")
                    .short('v')
                    .long("verbosity")
                    .value_parser([
                        PossibleValue::new("info"),
                        PossibleValue::new("debug"),
                        PossibleValue::new("trace"),
                        PossibleValue::new("warn"),
                        PossibleValue::new("error"),
                    ])
                    .default_value("warn")

            )
            .get_matches();
        // convert matches to LevelFilter
        matches.get_one("verbosity level").cloned()
            .map_or(LevelFilter::Warn, |level: String| {
                match level.as_str() {
                    "info" => LevelFilter::Info,
                    "debug" => LevelFilter::Debug,
                    "trace" => LevelFilter::Trace,
                    "warn" => LevelFilter::Warn,
                    "error" => LevelFilter::Error,
                    _ => LevelFilter::Warn,
            }
        })
    }

    pub fn setup_logging(&self) -> Result<(), fern::InitError> {
        fern::Dispatch::new()
            // Format the output
            .format(|out, message, record| {
                out.finish(format_args!(
                    "{}[{}:{}] {}: {}",
                    Local::now().format("[%Y-%m-%d %H:%M:%S]"), // Timestamp format
                    record.target(),
                    record.line().unwrap_or_default(),
                    record.level(),
                    message
                ))
            })
            // Set the default logging level
            .level(self.get_log_level())
            // Set the logging level for the `hyper` crate
            .level_for("mastodon_async", LevelFilter::Warn)
            .level_for("rustls", LevelFilter::Warn)
            // Output to stdout
            .chain(std::io::stdout())
            // Output to a log file
            .chain(fern::log_file("output.log")?)
            // Apply the configuration
            .apply()?;

        Ok(())
    }

    pub async fn main_loop(&self) -> Result<(), Box<dyn Error>> {
        log::info!("Main loop started");
        log::debug!("Loading config");
        let data = Config::from_json().to_mastodon_data();
        let mastodon = Mastodon::from(data);
        log::info!("Logging in to Mastodon");
        let you = mastodon.verify_credentials().await?;
        let user_id = &format!("{}", &you.id);
        log::info!("Logged in");
        log::debug!("Logged in as user id: {}", you.id);
        log::debug!("Waiting for mastodon events");
        let stream = mastodon.stream_user().await?;
        stream
            .try_for_each(|(event, _client)| async move {
                let user_id = user_id.clone();
                debug!("Event received:\n{:#?}", &event);
                match event {
                    Event::Update(update) => {
                        debug!("Update event received:\n{:#?}", &update);
                        if format!("{}", update.account.id) == user_id {
                            update.media_attachments.iter().cloned().for_each(|attachment| {
                                debug!("Attachment received:\n{:#?}", &attachment);
                                if attachment.media_type == MediaType::Image
                                    // unwrap is safe since is not none
                                    && (attachment.description.is_none() || attachment.description.as_ref().unwrap().is_empty())  {
                                    debug!("Attachment {} has no description", &attachment.id);

                                    // create new thread and run it in background
                                    tokio::spawn(async move {
                                        if let Some(url) = attachment.url.clone() {
                                            debug!("Generating description for attachment {} with URL: {}", attachment.id, attachment.url.unwrap());
                                            let description = Vision().get_description(url).await.unwrap_or_else(|err| {
                                                error!("Failed to generate description for attachment {}: {:#?}", attachment.id, err);
                                                "".to_string()
                                            });
                                            info!("Generated description for attachment {}: {}", attachment.id, description);
                                        } else {
                                            warn!("Cannot get URL for attachment {}", attachment.id);
                                        }

                                    });
                                }
                            });
                        }
                    }
                    Event::Notification(_) => {},
                    Event::Delete(_) => {},
                    Event::FiltersChanged => {},
                }
                Ok(())
            })
            .or_else(|e| async {
                error!("Error:\n{:#?}", e);
                Err(e)
            })
            .await?;

        Ok(())
    }
}
