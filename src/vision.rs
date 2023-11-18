use std::error::Error;

use log::debug;
use log::error;
use serde_json::{json, Value};
use voca_rs::strip::strip_tags;
pub struct Vision();

impl Vision {
    pub async fn get_description(
        &self,
        image_url: String,
        lang_code: String,
        context: String,
    ) -> Result<String, Box<dyn Error>> {
        let config = crate::config::Config::from_json();
        let client = reqwest::Client::new();
        let context = strip_tags(&context);
        let prompt = format!(
            "Please describe this image to visually impaired user.
        Please be as descriptive as possible, but keep it relatively short.
        You must write description in language with following two letter code: '{}'
        Use following context of message if needed: '{}'",
            lang_code, context
        );
        let prompt = textwrap::dedent(&prompt);
        debug!("Prompt: {}", &prompt);
        let response = client
            .post("https://api.openai.com/v1/chat/completions")
            .header("Content-Type", "application/json")
            .header(
                "Authorization",
                format!("Bearer {}", config.get_gpt_api_key()),
            )
            .json(&json!({
                "model": config.get_model(),
                "messages": [
                    {
                        "role": "user",
                        "content": [
                            {
                                "type": "text",
                                "text": prompt
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
            return Err(Box::new(std::io::Error::new(
                std::io::ErrorKind::Other,
                format!(
                    "ChatGPT API returned error, http code: {}",
                    response.status()
                ),
            )));
        }
        let body: Value = response.json().await?;
        debug!("Full response from ChatGPT API: {:#?}", body);
        let error = body
            .as_object()
            .ok_or("Cannot get body as JSON object")?
            .get("error")
            .ok_or("Cannot get error from JSON object");
        match error {
            Ok(error) => {
                error!("ChatGPT API returned error:\n{:#?}", error);
            }
            Err(_) => {
                error!("ChatGPT API returned unknown error.\n{:#?}", body);
            }
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
