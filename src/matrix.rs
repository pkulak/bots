use std::env;

use matrix_sdk::{Client, SyncSettings};
use matrix_sdk::ClientConfig;
use matrix_sdk::room::Joined;
use matrix_sdk::room::Room;
use matrix_sdk::ruma::{MxcUri, ServerName, UserId};
use matrix_sdk::ruma::events::AnyMessageEventContent;
use matrix_sdk::ruma::events::room::ImageInfo;
use matrix_sdk::ruma::events::room::member::MemberEventContent;
use matrix_sdk::ruma::events::room::message::{FileInfo, FileMessageEventContent, ImageMessageEventContent, MessageEventContent};
use matrix_sdk::ruma::events::room::message::MessageType;
use matrix_sdk::ruma::events::room::message::TextMessageEventContent;
use matrix_sdk::ruma::events::StrippedStateEvent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use reqwest::Url;
use rust_decimal::prelude::*;
use rusty_money::iso::Currency;
use rusty_money::Money;
use tokio::time;
use tokio::time::Duration;

pub async fn get_text_message(
    event: SyncMessageEvent<MessageEventContent>,
    room: Room,
    client: Client
) -> Option<(Joined, UserId, String)> {
    if let Room::Joined(room) = room {
        if let SyncMessageEvent {
            content: MessageEventContent {
                msgtype: MessageType::Text(TextMessageEventContent { body, .. }),
                ..
            },
            sender,
            ..
        } = event
        {
            if sender.eq(&client.user_id().await.unwrap()) {
                None
            } else {
                Some((room, sender, body))
            }
        } else {
            Option::None
        }
    } else {
        Option::None
    }
}

pub async fn get_image_message(
    event: SyncMessageEvent<MessageEventContent>,
    room: Room,
    client: Client
) -> Option<(Joined, UserId, MxcUri, Box<ImageInfo>)> {
    if let Room::Joined(room) = room {
        if let SyncMessageEvent {
            content: MessageEventContent {
                msgtype: MessageType::Image(
                    ImageMessageEventContent { url: Some(uri), info: Some(info), .. }),
                ..
            },
            sender,
            ..
        } = event
        {
            if sender.eq(&client.user_id().await.unwrap()) {
                None
            } else {
                Some((room, sender, uri, info))
            }
        } else {
            Option::None
        }
    } else {
        Option::None
    }
}

pub async fn get_file_message(
    event: SyncMessageEvent<MessageEventContent>,
    room: Room,
    client: Client
) -> Option<(Joined, UserId, MxcUri, Box<FileInfo>)> {
    if let Room::Joined(room) = room {
        if let SyncMessageEvent {
            content: MessageEventContent {
                msgtype: MessageType::File(
                    FileMessageEventContent { url: Some(uri), info: Some(info), .. }),
                ..
            },
            sender,
            ..
        } = event
        {
            if sender.eq(&client.user_id().await.unwrap()) {
                None
            } else {
                Some((room, sender, uri, info))
            }
        } else {
            Option::None
        }
    } else {
        Option::None
    }
}

pub fn find_command<'a>(prefixes: Vec<&str>, message: &'a str) -> Option<&'a str> {
    for prefix in &prefixes {
        if let Some(command) = get_command(prefix, message) {
            return Option::Some(command)
        }
    }

    Option::None
}

pub fn get_command<'a>(prefix: &str, message: &'a str) -> Option<&'a str> {
    let lower_message = message.to_lowercase();
    let lower_prefix = prefix.to_lowercase();

    if lower_message.eq(&lower_prefix) {
        return Some("")
    }

    if lower_message.starts_with(&format!("{} ", lower_prefix)) {
        return Some(message[prefix.len() + 1..].trim())
    }

    if lower_message.starts_with(&format!("{}. ", lower_prefix)) {
        return Some(message[prefix.len() + 2..].trim())
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

pub async fn create_client(bot_name: &str) -> anyhow::Result<Client> {
    let username = env::var("USERNAME")
        .expect("USERNAME environmental variable not set");

    let password = env::var("PASSWORD")
        .expect("PASSWORD environmental variable not set");

    let homeserver = env::var("HOMESERVER")
        .expect("HOMESERVER environmental variable not set");

    let mut config = dirs::config_dir().expect("no config directory found");
    config.push(bot_name);

    println!("saving configuration to {:?}", config);

    let client_config = ClientConfig::new().store_path(config);
    let homeserver_url = Url::parse(&homeserver).expect("invalid homeserver url");
    let client = Client::new_with_config(homeserver_url, client_config).unwrap();

    client.login(&username, &password, None, Some(bot_name)).await?;

    println!("logged in as {}", username);

    client.sync_once(SyncSettings::default()).await.unwrap();
    client.register_event_handler(on_room_invitation).await;

    Ok(client)
}

pub fn text_plain(message: &str) -> impl Into<AnyMessageEventContent> {
    AnyMessageEventContent::RoomMessage(MessageEventContent::text_plain(message))
}

pub fn text_html(plain: &str, html: &str) -> impl Into<AnyMessageEventContent> {
    AnyMessageEventContent::RoomMessage(MessageEventContent::text_html(plain, html))
}

pub fn normalize_sender(sender: UserId, command: &str) -> anyhow::Result<UserId> {
    let sender = if !command.is_empty() {
        create_user_id(command)?
    } else {
        sender
    };

    Ok(sender)
}

pub fn create_user_id(id: &str) -> anyhow::Result<UserId> {
    let id = id.to_lowercase();
    let id = id.trim();

    let id = if id == "dad" {
        UserId::try_from("@phil:kulak.us")?
    } else if id == "mom" {
        UserId::try_from("@gwen:kulak.us")?
    } else {
        UserId::parse_with_server_name(id, <&ServerName>::try_from("kulak.us")?)?
    };

    Ok(id)
}

pub fn pretty_user_id(user_id: &UserId) -> String {
    match user_id.as_str() {
        "@phil:kulak.us" => return "Dad".to_string(),
        "@gwen:kulak.us" => return "Mom".to_string(),
        _ => ()
    }

    let localpart = &mut user_id.localpart().to_string();

    if let Some(s) = localpart.get_mut(0..1) {
        s.make_ascii_uppercase()
    };

    localpart.to_string()
}

pub fn is_admin(user_id: &UserId) -> bool {
    user_id.as_ref().eq_ignore_ascii_case("@phil:kulak.us") ||
        user_id.as_ref().eq_ignore_ascii_case("@gwen:kulak.us")
}

pub fn money_to_i64(money: &Money<Currency>) -> i64 {
    (money.clone() * 100isize).amount().to_i64().unwrap()
}
