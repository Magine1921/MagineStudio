use std::time::Duration;

use futures_util::future::join3;
use reqwest::Client;
use serde_json::Value;

pub const MANAGED_URL: &str = "http://127.0.0.1:8188";
pub const DESKTOP_URL: &str = "http://127.0.0.1:8000";

const CANDIDATE_URLS: &[&str] = &[MANAGED_URL, DESKTOP_URL];
const FL2VA_MODELS: &[&str] = &[
    "minimax_h3_fl2va_pruned_int8_convrot.safetensors",
    "minimax_h3_fl2va_pruned_fp8_scaled.safetensors",
];
const TEXT_ENCODER: &str = "qwen3vl_32b_minimax_h3_nvfp4_awq.safetensors";
const AUDIO_VAE: &str = "minimax_h3_audio_vae_fp32.safetensors";
const VIDEO_VAE: &str = "minimax_h3_video_vae_fp16.safetensors";

#[derive(Clone, Debug)]
pub struct ApiModelStatus {
    pub url: String,
    pub found: usize,
    pub total: usize,
}

impl ApiModelStatus {
    pub fn ready(&self) -> bool {
        self.found == self.total
    }
}

pub async fn detect_base_url() -> Option<String> {
    let client = client(Duration::from_secs(2)).ok()?;
    for url in CANDIDATE_URLS {
        if is_ready_with_client(&client, url).await {
            return Some((*url).to_string());
        }
    }
    None
}

pub async fn is_ready_at(url: &str) -> bool {
    let Ok(client) = client(Duration::from_secs(2)) else {
        return false;
    };
    is_ready_with_client(&client, url).await
}

pub async fn detect_model_status() -> Option<ApiModelStatus> {
    let url = detect_base_url().await?;
    let client = client(Duration::from_secs(5)).ok()?;
    let (unet, clip, vae) = join3(
        loader_values(
            &client,
            &url,
            "UNETLoader",
            "/UNETLoader/input/required/unet_name/0",
        ),
        loader_values(
            &client,
            &url,
            "CLIPLoader",
            "/CLIPLoader/input/required/clip_name/0",
        ),
        loader_values(
            &client,
            &url,
            "VAELoader",
            "/VAELoader/input/required/vae_name/0",
        ),
    )
    .await;
    let found = [
        unet.as_ref()
            .is_some_and(|values| FL2VA_MODELS.iter().any(|name| contains(values, name))),
        clip.as_ref()
            .is_some_and(|values| contains(values, TEXT_ENCODER)),
        vae.as_ref()
            .is_some_and(|values| contains(values, AUDIO_VAE)),
        vae.as_ref()
            .is_some_and(|values| contains(values, VIDEO_VAE)),
    ]
    .into_iter()
    .filter(|ready| *ready)
    .count();

    Some(ApiModelStatus {
        url,
        found,
        total: 4,
    })
}

async fn is_ready_with_client(client: &Client, url: &str) -> bool {
    client
        .get(format!("{url}/system_stats"))
        .send()
        .await
        .is_ok_and(|response| response.status().is_success())
}

async fn loader_values(
    client: &Client,
    url: &str,
    node: &str,
    pointer: &str,
) -> Option<Vec<String>> {
    let response = client
        .get(format!("{url}/object_info/{node}"))
        .send()
        .await
        .ok()?;
    if !response.status().is_success() {
        return None;
    }
    let body: Value = response.json().await.ok()?;
    values_at(&body, pointer)
}

fn values_at(body: &Value, pointer: &str) -> Option<Vec<String>> {
    Some(
        body.pointer(pointer)?
            .as_array()?
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
    )
}

fn contains(values: &[String], expected: &str) -> bool {
    values.iter().any(|value| {
        value
            .rsplit(['/', '\\'])
            .next()
            .is_some_and(|name| name.eq_ignore_ascii_case(expected))
    })
}

fn client(timeout: Duration) -> Result<Client, reqwest::Error> {
    Client::builder().no_proxy().timeout(timeout).build()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn extracts_loader_inventory() {
        let body = json!({
            "UNETLoader": {
                "input": { "required": { "unet_name": [["folder/model.safetensors"]] } }
            }
        });
        assert_eq!(
            values_at(&body, "/UNETLoader/input/required/unet_name/0"),
            Some(vec!["folder/model.safetensors".to_string()])
        );
    }

    #[test]
    fn matches_loader_names_with_or_without_subfolders() {
        let values = vec!["unet/MODEL.SAFETENSORS".to_string()];
        assert!(contains(&values, "model.safetensors"));
    }
}
