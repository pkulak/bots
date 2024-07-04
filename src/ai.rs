use anyhow::{bail, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::env;

#[derive(Serialize)]
struct ImageBody<'a> {
    prompt: &'a str,
    n: usize,
    size: &'a str,
    model: &'a str,
    quality: &'a str,
}

#[derive(Deserialize)]
struct Data {
    url: String,
}

#[derive(Deserialize)]
struct ImageResponse {
    data: Vec<Data>,
}

#[derive(Serialize, Deserialize)]
struct Message {
    role: String,
    content: String,
}

#[derive(Serialize)]
struct MessageList {
    model: String,
    messages: Vec<Message>,
}

#[derive(Deserialize)]
struct Choice {
    message: Message,
}

#[derive(Deserialize)]
struct ChatResponse {
    choices: Vec<Choice>,
}

pub async fn chat(prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let auth = env::var("OPENAI_KEY").expect("OPENAI_KEY environmental variable not set");

    let body = MessageList {
        model: "gpt-4o".to_string(),
        messages: vec![Message {
            role: "user".to_string(),
            content: prompt.to_string(),
        }],
    };

    let response = client
        .post("https://api.openai.com/v1/chat/completions")
        .header("Authorization", format!("Bearer {}", auth))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "unexpected response status from Open AI: {}",
            response.status(),
        );
    }

    let body = response.json::<ChatResponse>().await?;

    Ok(body.choices.first().unwrap().message.content.clone())
}

pub async fn generate_image(prompt: &str) -> Result<Bytes> {
    let client = reqwest::Client::new();

    let auth = env::var("OPENAI_KEY").expect("OPENAI_KEY environmental variable not set");

    let body = ImageBody {
        prompt,
        n: 1,
        size: "1024x1024",
        model: "dall-e-3",
        quality: "hd",
    };

    let response = client
        .post("https://api.openai.com/v1/images/generations")
        .header("Authorization", format!("Bearer {}", auth))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "unexpected response status from Open AI: {}",
            response.status(),
        );
    }

    let body = response.json::<ImageResponse>().await?;
    let url = &body.data.first().unwrap().url;

    let image = client.get(url).send().await?.bytes().await?;

    Ok(image)
}
