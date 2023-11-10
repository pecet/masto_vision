use std::error::Error;

use async_openai::types::{ChatCompletionTool, CreateImageRequest};
use async_openai::{Client, config::Config};
use async_openai::{
    types::{
        ChatCompletionRequestAssistantMessageArgs, ChatCompletionRequestSystemMessageArgs,
        ChatCompletionRequestUserMessageArgs, CreateChatCompletionRequestArgs,
    },
};
use kv_log_macro::info;
use log::debug;
pub struct Vision();

impl Vision {
    pub async fn get_description(&self, image: String) -> Result<String, Box<dyn Error>> {
        let config = crate::config::Config::from_json();
        let gpt_config = config.to_gpt_config();
        let client = Client::with_config(gpt_config);

        let request = CreateChatCompletionRequestArgs::default()
            .max_tokens(512u16)
            .model(config.get_model())
            .messages([
                ChatCompletionRequestSystemMessageArgs::default()
                    .content("You are a helpful assistant. Please describe below image in short sentences to visually impaired people.")
                    .build()?
                    .into(),
                ChatCompletionRequestAssistantMessageArgs::default()
                    .content(format!("{}", &image))
                    .build()?
                    .into(),
            ])
            .build()?;
        let response = client.chat().create(request).await?;
        let response = response.choices[0].message.content.clone().unwrap();
        debug!("Got description from chat gpt: {}", response);

        Ok("".to_string())

    }
}