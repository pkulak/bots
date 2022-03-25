use anyhow::{bail, Result};
use serde::Serialize;
use std::env;

#[derive(Serialize)]
struct Body {
    what: String,
}

async fn webook(id: &str, message: &str) -> Result<()> {
    let url = format!("http://ha.kulak.us/api/webhook/{}", id);
    let body = Body {
        what: String::from(message),
    };

    let response = reqwest::Client::new()
        .post(url)
        .json(&body)
        .send()
        .await?;

    if !response.status().is_success() {
        bail!(
            "unexpected response status from Home Assistant: {}",
            response.status()
        );
    }

    Ok(())
}

pub async fn broadcast(message: &str) -> Result<()> {
    let id = env::var("BROADCAST")
        .expect("BROADCAST environmental variable not set");

    webook(&id, message).await?;
    Ok(())
}
