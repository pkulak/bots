use anyhow::{bail, Result};
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::{Client, SyncSettings};
use serde::Deserialize;

use crate::matrix;
use crate::webhook;

const TRIGGERS: &[&str] = &[
    "wow",
    "!",
    "amaz",
    "fantastic",
    "incredibl",
    "stun",
    "unbelievable",
    "fascinat",
    "marvelous",
    "shock",
    "surpris",
    "wonder",
    "owen",
    "lol",
];

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("owenbot").await?;

    client.register_event_handler(on_room_message).await;

    let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
    client.sync(settings).await;

    Ok(())
}

async fn on_room_message(event: SyncMessageEvent<MessageEventContent>, room: Room, client: Client) {
    if let Some((joined, _, message)) = matrix::get_text_message(event, room, client).await {
        let message = message.to_lowercase();

        for trigger in TRIGGERS {
            if message.contains(trigger) {
                joined.send(matrix::text_plain("Wow!"), None).await.unwrap();

                let wow = get_wow().await.unwrap();
                webhook::play_video(wow.as_str()).await.unwrap();
                return;
            }
        }
    }
}

#[derive(Deserialize)]
struct Body {
    video: Video,
}

#[derive(Deserialize)]
struct Video {
    #[serde(rename = "1080p")]
    large: String,
}

async fn get_wow() -> Result<String> {
    let response = reqwest::Client::new()
        .get("https://owen-wilson-wow-api.herokuapp.com/wows/random")
        .send()
        .await
        .unwrap();

    match response.status() {
        reqwest::StatusCode::OK => match response.json::<Vec<Body>>().await {
            Ok(parsed) => Ok(parsed.first().unwrap().video.large.clone()),
            Err(_) => bail!("unexpected response"),
        },
        _ => {
            bail!("unexpected status: {}", response.status())
        }
    }
}
