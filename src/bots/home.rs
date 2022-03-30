use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::{Client, SyncSettings};

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
    if let Some((_, _, message)) = matrix::get_text_message(event, room, client).await {
        if let Some(command) = matrix::get_command("bc", &message) {
            webhook::broadcast(command).await.unwrap()
        }

        if let Some(command) = matrix::get_command("n", &message) {
            webhook::notify(command).await.unwrap()
        }
    }
}
