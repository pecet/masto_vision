use log::*;

use kv_log_macro as log;
use masto_vision::handler::Handler;
#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let handler = Handler {};
    let _ = handler.setup_logging();
    info!("Starting MastoVision!");
    handler.main_loop().await.unwrap_or_else(|err| {
        error!("Critical error\n{:#?}", err);
    });
}
