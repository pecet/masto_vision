use std::error::Error;

use crate::config::Config;
use chrono::Local;
use futures_util::{TryFutureExt, TryStreamExt};
use log::{error, warn, LevelFilter};
use mastodon_async::prelude::*;

pub struct Handler();
impl Handler {
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
            .level(LevelFilter::Trace)
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
        log::info!("Logged in");
        log::debug!("Logged in as user id: {}", you.id);
        log::debug!("Waiting for mastodon events");
        let stream = mastodon.stream_user().await?;
        stream
            .try_for_each(|(event, _client)| async move {
                warn!("Event received:\n{:#?}", &event);
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
