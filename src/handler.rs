use std::{collections::HashMap, error::Error};

use crate::{config::Config, mastodon_patch::MastodonPatch, vision::Vision};
use chrono::Local;

use futures_util::{TryFutureExt, TryStreamExt};
use kv_log_macro::warn;
use log::{debug, error, info, LevelFilter};
use mastodon_async::{prelude::*, Mastodon};

use clap::{builder::PossibleValue, Arg};

pub struct Handler();
impl Handler {
    fn get_log_level(&self) -> LevelFilter {
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
                        PossibleValue::new("quiet"),
                    ])
                    .default_value("info"),
            )
            .get_matches();
        // convert matches to LevelFilter
        matches
            .get_one("verbosity level")
            .cloned()
            .map_or(LevelFilter::Info, |level: String| match level.as_str() {
                "info" => LevelFilter::Info,
                "debug" => LevelFilter::Debug,
                "trace" => LevelFilter::Trace,
                "warn" => LevelFilter::Warn,
                "error" => LevelFilter::Error,
                "quiet" => LevelFilter::Off,
                _ => LevelFilter::Info,
            })
    }

    pub fn setup_logging(&self) -> Result<(), fern::InitError> {
        let log_level = self.get_log_level();

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
            .level(log_level)
            // Set the logging level for the `hyper` crate
            .level_for("mastodon_async", LevelFilter::Warn)
            .level_for("rustls", LevelFilter::Warn)
            // Output to stdout
            .chain(std::io::stdout())
            // Output to a log file
            .chain(fern::log_file("output.log")?)
            // Apply the configuration
            .apply()?;
        info!("Log level set to: {}", log_level);
        Ok(())
    }

    #[allow(unreachable_code)]
    pub async fn main_loop(&self) -> Result<(), Box<dyn Error>> {
        log::info!("Main loop started");
        log::debug!("Loading config");
        let config = Config::from_json();
        let data = config.to_mastodon_data();
        let mastodon = Mastodon::from(data);
        let _mastodon_patch = MastodonPatch::new(config);
        //let mastodon_patch_ref = &mastodon_patch;
        log::info!("Logging in to Mastodon");
        let you = mastodon.verify_credentials().await?;
        let user_id = &format!("{}", &you.id);
        log::info!("Logged in");
        log::debug!("Logged in as user id: {}", you.id);
        let mut counter = 0_u64;

        loop {
            counter += 1;
            debug!("Waiting for mastodon events (try: {})", counter);
            let stream = mastodon.stream_user().await?;
            stream
                .try_for_each(|(event, _client)| async move {
                    let user_id = user_id.clone();
                    debug!("Event received:\n{:#?}", &event);
                    if let Event::Update(update) = event {
                        debug!("Update event received:\n{:#?}", &update);
                        if format!("{}", update.account.id) == user_id {
                            let attachments: Vec<_> = update.media_attachments.into_iter().collect();
                            let handles: Vec<_> = attachments.into_iter().map(|attachment| {
                                tokio::spawn(async move {
                                    if attachment.media_type == MediaType::Image &&
                                        (attachment.description.is_none() || attachment.description.unwrap().is_empty()) {
                                            if let Some(url) = attachment.url.clone() {
                                                debug!("Generating description for attachment {} with URL: {}", attachment.id, attachment.url.unwrap());
                                                let description = Vision().get_description(url).await.unwrap_or_else(|err| {
                                                    error!("Failed to generate description for attachment {}: {:#?}", attachment.id, err);
                                                    "".to_string()
                                                });
                                                info!("Generated description for attachment {}: {}", attachment.id, description);
                                                return (attachment.id.clone(), Some(description))
                                            } else {
                                                warn!("Cannot get URL for attachment {}", attachment.id);
                                            }
                                    }
                                    (attachment.id.clone(), None)
                                })
                            }).collect();
                            let results = futures_util::future::join_all(handles).await
                                .into_iter()
                                .map(|res| res.expect("Task panicked"));
                            let descriptions: Vec<_> = results.collect();
                            debug!("Number of descriptions got: {}", descriptions.len());
                            let descriptions_filtered: HashMap<_, _> = descriptions.into_iter()
                                .filter(|d| d.1.is_some())
                                .map(|d| (d.0.to_string(), d.1.unwrap()))
                                .collect();
                            debug!("Number of non-empty descriptions got: {}", descriptions_filtered.len());
                            // TODO: avoid creating new instance of Config here
                            let mp = MastodonPatch::new(Config::from_json());
                            let message_id = update.id.to_string();
                            let current_json = mp.get_json_of_message(message_id.clone())
                                .await.unwrap_or_default().unwrap_or_default();
                            mp.put_json_of_message(current_json, message_id.clone(), descriptions_filtered).await;
                            info!("Successfully added description to message {}", message_id);
                        }
                    }
                    Ok(())
                })
                .unwrap_or_else(
                    |e| error!("Ignoring error while streaming: \n{:#?}", e)
                ).await;
        }
        // This will never be reached
        Ok(())
    }
}
