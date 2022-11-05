use bytes::Buf;
use matrix_sdk::{Client, SyncSettings};
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
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

async fn handle_message(joined: Joined, message: &String) {
    if let Some(prompt) = matrix::get_command("show me", &message) {
        joined.send(matrix::text_plain("Let's see..."), None).await.unwrap();

        let image = match ai::generate_image(prompt).await {
            Ok(image) => image,
            Err(e) => {
                println!("Error creating image: {}", e);

                joined.send(matrix::text_plain("Oh no! I couldn't do it. :("), None)
                    .await.unwrap();
                return;
            }
        };

        joined.send_attachment("image.png", &mime::IMAGE_PNG, &mut image.reader(), None)
            .await
            .unwrap();
    }
}
