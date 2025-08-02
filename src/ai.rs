use anyhow::{bail, Result};
use bytes::Bytes;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    env,
    sync::{LazyLock, Mutex},
};

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

#[derive(Serialize, Deserialize, Clone)]
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

static ALL_CONTEXT: LazyLock<Mutex<HashMap<String, Vec<Message>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn get_context(room: &str, prompt: &str) -> Vec<Message> {
    let mut context = ALL_CONTEXT.lock().unwrap();
    let messages = context.entry(room.to_string()).or_insert(vec![Message {
        role: "developer".to_string(),
        content: "Respond as concisely as possible, unless asked to expand.".to_string(),
    }]);

    messages.push(Message {
        role: "user".to_string(),
        content: prompt.to_string(),
    });

    messages.clone()
}

fn record_response(room: &str, response: String) {
    let mut context = ALL_CONTEXT.lock().unwrap();
    let Some(messages) = context.get_mut(room) else {
        return;
    };

    messages.push(Message {
        role: "assistant".to_string(),
        content: response.to_string(),
    });
}

fn cleanup_context(room: &str) {
    let mut context = ALL_CONTEXT.lock().unwrap();
    let Some(messages) = context.get_mut(room) else {
        return;
    };

    while messages.len() > 10 {
        messages.remove(1);
    }
}

pub async fn chat(room: &str, prompt: &str) -> Result<String> {
    let client = reqwest::Client::new();

    let auth = env::var("OPENAI_KEY").expect("OPENAI_KEY environmental variable not set");

    let body = MessageList {
        model: "gpt-4.1".to_string(),
        messages: get_context(room, prompt),
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
    let response = body.choices.first().unwrap().message.content.clone();

    record_response(room, response.clone());
    cleanup_context(room);

    Ok(response)
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
