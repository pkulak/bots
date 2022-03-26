use matrix_sdk::{Client, SyncSettings};
use matrix_sdk::ClientConfig;
use matrix_sdk::room::Joined;
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::member::MemberEventContent;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
use matrix_sdk::ruma::events::StrippedStateEvent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use reqwest::Url;
use tokio::time;
use tokio::time::Duration;

pub async fn get_text_message(
    event: SyncMessageEvent<MessageEventContent>,
    room: Room
) -> Option<(Joined, String)> {
    if let Room::Joined(room) = room {
        if let SyncMessageEvent {
            content: MessageEventContent {
                msgtype: MessageType::Text(TextMessageEventContent { body: msg_body, .. }),
                ..
            },
            ..
        } = event
        {
            Option::Some((room, msg_body))
        } else {
            Option::None
        }
    } else {
        Option::None
    }
}

pub async fn get_command<'a>(prefix: &str, message: &'a str) -> Option<&'a str> {
    let lower_message = message.to_lowercase();
    let lower_prefix = prefix.to_lowercase();

    if lower_message.starts_with(&format!("{} ", lower_prefix)) {
        return Some(&message[prefix.len() + 1..])
    }

    if lower_message.starts_with(&format!("{}. ", lower_prefix)) {
        return Some(&message[prefix.len() + 2..])
    }

    Option::None
}

async fn on_room_invitation(
    room_member: StrippedStateEvent<MemberEventContent>,
    client: Client,
    room: Room,
) {
    if room_member.state_key != client.user_id().await.unwrap() {
        return;
    }

    if let Room::Invited(room) = room {
        println!("Autojoining room {}", room.room_id());
        let mut delay = 2;

        while let Err(err) = room.accept_invitation().await {
            // retry autojoin due to synapse sending invites, before the
            // invited user can join for more information see
            // https://github.com/matrix-org/synapse/issues/4345
            eprintln!("Failed to join room {} ({:?}), retrying in {}s", room.room_id(), err, delay);

            time::sleep(Duration::from_secs(delay)).await;
            delay *= 2;

            if delay > 3600 {
                eprintln!("Can't join room {} ({:?})", room.room_id(), err);
                break;
            }
        }
        println!("Successfully joined room {}", room.room_id());
    }
}

pub async fn create_client(
    bot_name: &str,
    homeserver_url: &str,
    username: &str,
    password: &str
) -> anyhow::Result<Client> {
    let mut home = dirs::config_dir().expect("no config directory found");
    home.push(bot_name);

    println!("saving configuration to {:?}", home);

    let client_config = ClientConfig::new().store_path(home);
    let homeserver_url = Url::parse(homeserver_url).expect("invalid homeserver url");
    let client = Client::new_with_config(homeserver_url, client_config).unwrap();

    client.login(username, password, None, Some(bot_name)).await?;

    println!("logged in as {}", username);

    client.sync_once(SyncSettings::default()).await.unwrap();
    client.register_event_handler(on_room_invitation).await;

    Ok(client)
}
