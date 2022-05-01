use anyhow::{bail, Result};
use serde::Serialize;
use std::env;

#[derive(Serialize)]
struct Body<'a> {
    what: &'a str,
}

async fn webook(id: &str, message: &str) -> Result<()> {
    let url = format!("http://ha.kulak.us/api/webhook/{}", id);
    let body = Body {
        what: message,
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

pub async fn play_video(url: &str) -> Result<()> {
    let id = env::var("PLAY_VIDEO")
        .expect("PLAY_VIDEO environmental variable not set");

    println!("playing video at {}", url);

    webook(&id, url).await?;
    Ok(())
}

pub async fn broadcast(message: &str) -> Result<()> {
    let id = env::var("BROADCAST")
        .expect("BROADCAST environmental variable not set");

    println!("broadcasting {}", message);

    webook(&id, message).await?;
    Ok(())
}

pub async fn notify(message: &str) -> Result<()> {
    let id = env::var("NOTIFY")
        .expect("NOTIFY environmental variable not set");

    println!("notifying {}", message);

    webook(&id, message).await?;
    Ok(())
}
