use mastodon_async::Data;
use serde::{Deserialize, Serialize};
use async_openai::config::OpenAIConfig;

#[derive(Debug, Deserialize, Serialize)]
struct MastodonConfig {
    base_url: String,
    client_id: String,
    client_secret: String,
    access_token: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct GptConfig {
    access_token: String,
    model: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    mastodon: MastodonConfig,
    gpt: GptConfig,
}

impl Config {
    pub fn from_json() -> Self {
        let config = std::fs::read_to_string("config.json").unwrap_or_else(|_| {
            panic!("Failed to read config.json. Please make sure it exists and is readable.")
        });
        serde_json::from_str(&config).unwrap_or_else(|_| {
            panic!("Failed to parse config.json. Please make sure it is valid JSON.")
        })
    }
    pub fn to_mastodon_data(&self) -> Data {
        Data {
            base: self.mastodon.base_url.clone().into(),
            client_id: self.mastodon.client_id.clone().into(),
            client_secret: self.mastodon.client_secret.clone().into(),
            redirect: "".into(),
            token: self.mastodon.access_token.clone().into(),
        }
    }
    pub fn to_gpt_config(&self) -> OpenAIConfig {
        OpenAIConfig::new()
            .with_api_key(self.gpt.access_token.clone())
    }
    pub fn get_model(&self) -> String {
        self.gpt.model.clone()
    }
    pub fn get_gpt_api_key(&self) -> String {
        self.gpt.access_token.clone()
    }
}
