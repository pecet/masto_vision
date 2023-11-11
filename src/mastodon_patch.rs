use log::{debug};
use serde_json::{json, from_value, to_value, Map};
use std::{error::Error, collections::HashMap};
use json_patch::{Patch, patch, merge};
use voca_rs::strip::strip_tags;

#[derive(Debug, Clone)]
pub struct MastodonPatch {
    config: crate::config::Config,
}

// implement methods which are currently not supported by MastodonAsync
impl MastodonPatch {
    pub fn new(config: crate::config::Config) -> Self {
        Self {
            config,
        }
    }

    // this does not work despite mastodon API reference tells otherwise :(
    pub async fn change_image_description(&self, image_id: String, image_description: String) -> Result<bool, Box<dyn Error>> {
        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/media/{}", self.config.get_mastodon_base_url(), image_id);
        debug!("Trying to PUT new image description: {}", &url);
        let mut params = HashMap::new();
        params.insert("description", image_description.clone());
        let response = client.put(url)
            .header("Authorization", format!("Bearer {}", self.config.get_mastodon_access_token()))
            .json(&json!(
                {
                    "description": image_description
                }
            ))
            .send().await?;
        debug!("Response from API: {:#?}", &response);
        let is_success = response.status().is_success();
        debug!("Success response?: {}", is_success);
        Ok(is_success)
    }

    pub async fn get_json_of_message(&self, message_id: String) -> Result<Option<String>, Box<dyn Error>> {
        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/statuses/{}", self.config.get_mastodon_base_url(), message_id);
        debug!("Trying to GET message: {}", &url);
        let response = client.get(url)
            .header("Authorization", format!("Bearer {}", self.config.get_mastodon_access_token()))
            .header("Content-Type", "application/json")
            .send().await?;
        if response.status().is_success() {
            let body = response.text().await?;
            debug!("Response from API: {:#?}", &body);
            return Ok(Some(body))
        }
        Ok(None)
    }

    pub async fn put_json_of_message(&self, json_string: String, message_id: String, image_id_with_description: HashMap<String, String>) {
        let client = reqwest::Client::new();
        let url = format!("{}/api/v1/statuses/{}", self.config.get_mastodon_base_url(), message_id);
        let previous_json = serde_json::from_str::<serde_json::Value>(&json_string).unwrap();
        let previous_json = previous_json.as_object().unwrap();
        debug!("Previous json: {:#?}", &previous_json);
        let empty = &json!("");
        // don't ask me why you cannot get message in exact form it was posted but instead it returns html
        // while it expects plain text in update
        let content = previous_json.get("content").unwrap_or(empty).as_str().unwrap_or_default();
        let content = content.replace("<br />", "\n");
        let content = strip_tags(&content);
        let mut media_ids: Vec<String> = Vec::new();
        let mut media_attachments = previous_json.get("media_attachments")
            .unwrap_or(empty).as_array()
            .cloned().unwrap_or_default();
        media_attachments.iter_mut().for_each(
            |attachment| {
                let attachment = attachment.as_object_mut().unwrap();
                let id = attachment.get("id").unwrap().as_str().unwrap().to_string();
                if image_id_with_description.contains_key(&id) {
                    let description = image_id_with_description.get(&id).unwrap();
                    attachment.insert("description".to_string(), description.clone().into());
                    media_ids.push(id);
                }
            }
        );

        let patched_json = json!(
            {
                "status": content,
                "in_reply_to_id": previous_json.get("in_reply_to_id"),
                "media_ids": media_ids,
                "media_attributes": media_attachments,
                "sensitive": previous_json.get("sensitive"),
                "spoiler_text": previous_json.get("spoiler_text"),
                "visibility": previous_json.get("visibility"),
                "poll": null,
                "language": previous_json.get("language"),
            }
        );
        debug!("Patched json: {:#?}", &patched_json);
        let response = client.put(url)
            .header("Authorization", format!("Bearer {}", self.config.get_mastodon_access_token()))
            .header("Content-Type", "application/json")
            .json(&patched_json)
            .send().await;
        match response {
            Ok(response) => {
                if response.status().is_success() {
                    debug!("Successfull message put {}", message_id);
                } else {
                    debug!("Failed to put message {} - status {}:\n{}", message_id, response.status(), response.text().await.unwrap_or_default());
                }
            },
            Err(err) => {
                debug!("Failed to put message {}: {:#?}", message_id, err);
            }
        }
    }
}