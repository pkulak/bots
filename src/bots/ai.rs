use bytes::Buf;
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::{Client, SyncSettings};
use mime;

use crate::ai;
use crate::matrix;

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("aibot").await?;

    client.register_event_handler(on_room_message).await;

    let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
    client.sync(settings).await;

    Ok(())
}

async fn on_room_message(event: SyncMessageEvent<MessageEventContent>, room: Room, client: Client) {
    if let Some((joined, _, message)) = matrix::get_text_message(event, room, client).await {
        handle_message(joined, &message).await;
    }
}

async fn handle_message(joined: Joined, message: &str) {
    let private_room = joined.members_no_sync().await.unwrap().len() <= 2;

    if let Some(prompt) = matrix::find_command(
        vec!["show me", "sherman, show me", "sherman show me"],
        message,
    ) {
        joined
            .send(matrix::text_plain("Let's see..."), None)
            .await
            .unwrap();

        let image = match ai::generate_image(prompt).await {
            Ok(image) => image,
            Err(e) => {
                println!("Error creating image: {}", e);

                joined
                    .send(matrix::text_plain("Oh no! I couldn't do it. :("), None)
                    .await
                    .unwrap();

                return;
            }
        };

        joined
            .send_attachment("image.png", &mime::IMAGE_PNG, &mut image.reader(), None)
            .await
            .unwrap();
    } else if let Some(prompt) = matrix::find_command(vec!["sherman,", "sherman"], message) {
        let response = match ai::chat(prompt).await {
            Ok(resp) => resp,
            Err(e) => {
                println!("Error with chat: {}", e);

                joined
                    .send(matrix::text_plain("I have no words. :("), None)
                    .await
                    .unwrap();

                return;
            }
        };

        joined
            .send(matrix::text_plain(&response), None)
            .await
            .unwrap();

        return;
    } else if joined.display_name().await.unwrap_or("".to_string()) == "AI Chat" || private_room {
        // we won't get involved if the conversation is about us
        if !private_room && message.to_lowercase().contains("sherman") {
            return;
        }

        let response = match ai::chat(message).await {
            Ok(resp) => resp,
            Err(e) => {
                println!("Error with chat: {}", e);

                joined
                    .send(matrix::text_plain("I have no words. :("), None)
                    .await
                    .unwrap();

                return;
            }
        };

        joined
            .send(matrix::text_plain(&response), None)
            .await
            .unwrap();

        return;
    }
}
