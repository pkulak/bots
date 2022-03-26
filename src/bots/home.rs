use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::SyncSettings;

use crate::matrix;
use crate::webhook;

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("homebot").await?;

    client.register_event_handler(on_room_message).await;

    let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
    client.sync(settings).await;

    Ok(())
}

async fn on_room_message(event: SyncMessageEvent<MessageEventContent>, room: Room) {
    if let Some((_, message)) = matrix::get_text_message(event, room).await {
        if let Some(command) = matrix::get_command("bc", &message).await {
            webhook::broadcast(command).await.unwrap()
        }

        if let Some(command) = matrix::get_command("n", &message).await {
            webhook::notify(command).await.unwrap()
        }
    }
}
