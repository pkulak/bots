use std::env;
use anyhow::{bail, Result};
use bytes::Bytes;
use serde::{Serialize, Deserialize};

#[derive(Serialize)]
struct Body<'a> {
    prompt: &'a str,
    n: usize,
    size: &'a str
}

#[derive(Deserialize)]
struct Data {
    url: String
}

#[derive(Deserialize)]
struct Response {
    data: Vec<Data>
}

pub async fn generate_image(prompt: &str) -> Result<Bytes> {
    let client = reqwest::Client::new();

    let auth = env::var("OPENAI_KEY")
        .expect("OPENAI_KEY environmental variable not set");

    let body = Body {
        prompt,
        n: 1,
        size: "1024x1024"
    };

    let response = client
        .post("https://api.openai.com/v1/images/generations")
        .header("Authorization", format!("Bearer {}", auth))
        .header("Content-Type", "application/json")
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        bail!("unexpected response status from Open AI: {}", response.status());
    }

    let body = response.json::<Response>().await?;
    let url = &body.data.first().unwrap().url;

    let image = client.get(url).send().await?.bytes().await?;

    Ok(image)
}