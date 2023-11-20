use std::time::Duration;
use std::{collections::HashMap, error::Error, sync::Arc};

use crate::shared_data::SHARED_DATA;
use crate::{config::Config, mastodon_patch::MastodonPatch, vision::Vision};
use chrono::Local;

use futures_util::{StreamExt, TryFutureExt, TryStreamExt};
use kv_log_macro::warn;
use log::{debug, error, info, LevelFilter};
use mastodon_async::entities::event::Event;
use mastodon_async::{prelude::*, Mastodon};

use mastodon_async::entities::status::Status;

use clap::{builder::PossibleValue, Arg};

#[derive(Clone)]
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
                let thread_id =
                    format!("{:?}", std::thread::current().id()).replace("ThreadId", "");
                out.finish(format_args!(
                    "{}{}[{}:{}] {}: {}",
                    Local::now().format("[%Y-%m-%d %H:%M:%S]"), // Timestamp format
                    thread_id,
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

    async fn handle_update(&self, update: Status, user_id: String) {
        debug!("Update event received:\n{:#?}", &update);
        { // if it does not contain this update, then make sure
            // that we don't block mutex until needed
            let already_parsed = SHARED_DATA.lock().unwrap().already_parsed.clone();
            if already_parsed.contains(&update.id.to_string()) {
                debug!("Already handled update, skipping");
                return;
            }
        }
        let lang = update.language.clone().unwrap_or("en".to_string());
        let context = update.content.clone();
        let lang_arc = Arc::new(lang.clone());
        let context_arc = Arc::new(context.clone());
        if format!("{}", update.account.id) == user_id {
            let attachments: Vec<_> = update.media_attachments.clone().into_iter().collect();
            let handles: Vec<_> = attachments.into_iter().map(|attachment| {
                let lang_arc_clone = lang_arc.clone();
                let context_arc = context_arc.clone();
                tokio::spawn(async move {
                    if attachment.media_type == MediaType::Image &&
                        (attachment.description.is_none() || attachment.description.unwrap().is_empty()) {
                            if let Some(url) = attachment.url.clone() {
                                let lang = lang_arc_clone.as_ref().clone();
                                let context = context_arc.as_ref().clone();
                                let mut retry: u64 = 0;
                                let attachment_id = attachment.id.clone();
                                let attachment_url = attachment.url.clone().unwrap();
                                loop {
                                    retry += 1;
                                    debug!("Generating description for attachment {} with URL: {}", &attachment_id, &attachment_url);
                                    debug!("Retry: {}", retry);
                                    let result = Vision().get_description(url.clone(), lang.clone(), context.clone()).await;
                                    match result {
                                        Ok(ref description) => {
                                            info!("Generated description for attachment {}: {}", attachment.id, description);
                                            return (attachment.id.clone(), Some(description.clone()))
                                        },
                                        Err(ref err) => {
                                            error!("Failed to generate description for attachment {}: {:#?}", attachment.id, err);
                                            if retry >= 10 {
                                                error!("Maximum retry count reached, giving up");
                                                return (attachment.id.clone(), None);
                                            }
                                            error!("Retrying after slight delay");
                                            std::thread::sleep(Duration::from_millis(2000));
                                        }
                                    };
                                }

                            } else {
                                warn!("Cannot get URL for attachment {}", attachment.id);
                            }
                    }
                    (attachment.id.clone(), None)
                })
            }).collect();
            let results = futures_util::future::join_all(handles)
                .await
                .into_iter()
                .map(|res| res.expect("Task panicked"));
            let descriptions: Vec<_> = results.collect();
            debug!("Number of descriptions got: {}", &descriptions.len());
            let descriptions_filtered: HashMap<_, _> = descriptions
                .iter()
                .filter(|d| d.1.is_some())
                .map(|d| (d.0.to_string(), d.1.clone().unwrap()))
                .collect();
            debug!(
                "Number of non-empty descriptions got: {}",
                descriptions_filtered.len()
            );
            if descriptions.len() == descriptions_filtered.len() {
                debug!("All descriptions generated successfully");
                let mut shared_data = SHARED_DATA.lock().unwrap();
                shared_data.already_parsed.insert(update.id.to_string());
                shared_data.save();
                debug!("Saved parsed status ID to shared data");
                debug!("Current parsed status IDs: {:#?}", SHARED_DATA.lock().unwrap().already_parsed);
            } else {
                debug!(
                    "Some descriptions failed to generate: {}",
                    descriptions.len() - descriptions_filtered.len()
                );
            }
            let message_id = update.clone().id.to_string();
            if descriptions_filtered.is_empty() {
                debug!("No descriptions generated for message {}", message_id);
                return;
            }
            // TODO: avoid creating new instance of Config here
            let mp = MastodonPatch::new(Config::from_json());
            let current_json = mp
                .get_json_of_message_with_retry(message_id.clone(), 10)
                .await
                .unwrap_or_default()
                .unwrap_or_default();
            mp.put_json_of_message_with_retry(
                current_json,
                message_id.clone(),
                descriptions_filtered,
                10,
            )
            .await
            .unwrap();
            info!("Successfully added description to message {}", message_id);
        }
    }

    pub async fn run(&self) -> Result<(), Box<dyn Error>> {
        let self_arc = Arc::new(self.clone());
        let self_clone = self_arc.clone();
        let self_clone2 = self_arc.clone();
        let streaming_loop = tokio::spawn(async move {
            self_clone
                .streaming_loop()
                .unwrap_or_else(|err| {
                    error!("Critical error in streaming loop\n{:#?}", err);
                })
                .await;
        });
        let manual_loop = tokio::spawn(async move {
            std::thread::sleep(Duration::from_secs(10));
            self_clone2
                .manual_loop()
                .unwrap_or_else(|err| {
                    error!("Critical error in streaming loop\n{:#?}", err);
                    panic!("{:#?}", err);
                })
                .await;
        });
        let _ = tokio::join!(streaming_loop, manual_loop);
        Ok(())
    }

    #[allow(unreachable_code)]
    pub async fn manual_loop(&self) -> Result<(), Box<dyn Error>> {
        log::info!("Manual loop started");
        log::debug!("Loading config");
        let config = Config::from_json();
        let manual = config.get_manual_refresh_config();
        if !manual.enabled {
            log::info!("Manual refresh disabled, skipping");
            return Ok(());
        }
        let data = config.to_mastodon_data();
        let mastodon = Mastodon::from(data);
        log::info!("Logging in to Mastodon");
        let you = mastodon.verify_credentials().await?;
        let user_id = &format!("{}", &you.id);
        log::info!("Logged in");
        log::debug!("Logged in as user id: {}", you.id);

        let mut initial = true;
        std::thread::sleep(Duration::from_secs(manual.initial_delay));
        loop {
            log::info!("Manually refreshing statuses");
            let mut request = StatusesRequest::new();
            request.only_media();
            let statuses = if initial {
                manual.initial_statuses
            } else {
                manual.statuses
            };
            request.limit(statuses);
            let statuses = mastodon.statuses(&you.id, request).await?;
            let iter = statuses.items_iter();
            iter.for_each(|status| async move {
                self.handle_update(status.clone(), user_id.clone()).await;
                std::thread::sleep(Duration::from_secs(1));
            })
            .await;
            std::thread::sleep(Duration::from_secs(manual.interval));
            initial = false;
        }
        Ok(())
    }

    #[allow(unreachable_code)]
    pub async fn streaming_loop(&self) -> Result<(), Box<dyn Error>> {
        log::info!("Streaming loop started");
        log::debug!("Loading config");
        let config = Config::from_json();
        let data = config.to_mastodon_data();
        let mastodon = Mastodon::from(data);
        let streaming = config.get_streaming_config();
        if !streaming.enabled {
            log::info!("Streaming disabled, skipping");
            return Ok(());
        }
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
                        let self_clone = self.clone();
                        let update_clone = update.clone();
                        let user_id_clone = user_id.clone();
                        tokio::spawn(async move {
                            self_clone.handle_update(update_clone, user_id_clone).await;
                        });
                    }
                    Ok(())
                })
                .unwrap_or_else(|e| error!("Ignoring error while streaming: \n{:#?}", e))
                .await;
        }
        // This will never be reached
        Ok(())
    }
}
