use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::{Client, SyncSettings};
use std::time::Duration;

use crate::matrix;
use crate::webhook;

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("homebot").await?;

    client.register_event_handler(on_room_message).await;

    let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
    client.sync(settings).await;

    Ok(())
}

async fn on_room_message(event: SyncMessageEvent<MessageEventContent>, room: Room, client: Client) {
    if let Some((joined, _, message)) = matrix::get_text_message(event, room, client).await {
        handle_message(&message).await;

        if message.to_lowercase().starts_with("in ") {
            let parts: Vec<&str> = message.split(' ').collect();

            if parts.len() < 4 {
                return;
            }

            let minutes = match parts[1].parse::<u64>() {
                Ok(n) => n,
                Err(_) => return,
            };

            let unit = parts[2].to_lowercase();

            if unit.contains("second")
                || unit.contains("hour")
                || unit.contains("day")
                || unit.contains("week")
                || unit.contains("month")
                || unit.contains("year")
            {
                joined
                    .send(
                        matrix::text_plain("Sorry, only minutes are supported right now"),
                        None,
                    )
                    .await
                    .unwrap();
                return;
            }

            let command = if unit.contains("minute") {
                parts[3..].to_vec()
            } else {
                parts[2..].to_vec()
            };

            let response = if minutes == 1 {
                "See you in a minute!".to_string()
            } else {
                format!("See you in {} minutes!", minutes)
            };

            joined
                .send(matrix::text_plain(&response), None)
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_secs(minutes * 60)).await;
            handle_message(&command.join(" ")).await;
        }
    }
}

async fn handle_message(message: &str) {
    if let Some(command) = matrix::find_command(vec!["bc", "broadcast", "say"], message) {
        webhook::broadcast(command).await.unwrap()
    }

    if let Some(command) = matrix::find_command(vec!["n", "notify"], message) {
        webhook::notify(command).await.unwrap()
    }
}
