use std::error::Error;

use log::debug;
use serde_json::{json, Value};
pub struct Vision();

impl Vision {
    pub async fn get_description(&self, image_url: String, lang_code: String, context: String) -> Result<String, Box<dyn Error>> {
        let config = crate::config::Config::from_json();
        let client = reqwest::Client::new();

        let response = client.post("https://api.openai.com/v1/chat/completions")
            .header("Content-Type", "application/json")
            .header("Authorization", format!("Bearer {}", config.get_gpt_api_key()))
            .json(&json!({
                "model": config.get_model(),
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": format!("Please describe this image to visually impaired user.
                                Please be as descriptive as possible, but keep it relatively short.
                                You must write description in language with following two letter code: '{}'
                                Use following context of message if needed:
                                '{}'", lang_code, context)
                            },
                            {
                                "type": "image_url",
                                "image_url": {
                                    "url": image_url
                                }
                            }
                        ]
                    }
                ],
                "max_tokens": config.get_max_tokens(),
            }))
            .send()
            .await?;
        if !response.status().is_success() {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "ChatGPT API returned error")));
        }
        let body: Value = response.json().await?;
        debug!("Full response from ChatGPT API: {:#?}", body);
        if body.as_object().unwrap().get("error").is_some() {
            return Err(Box::new(std::io::Error::new(std::io::ErrorKind::Other, "ChatGPT API returned error")));
        }
        let choices = body
            .as_object()
            .unwrap()
            .get("choices")
            .unwrap()
            .as_array()
            .unwrap();
        let first_choice = choices.first().unwrap().as_object().unwrap();
        let message = first_choice.get("message").unwrap().as_object().unwrap();
        let content = message.get("content").unwrap().as_str().unwrap();
        Ok(content.to_string())
    }
}
