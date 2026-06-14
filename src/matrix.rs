use bytes::Bytes;
use std::env;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::events::room::member::StrippedRoomMemberEvent;
use matrix_sdk::ruma::events::room::message::{
    FileInfo, FileMessageEventContent, ImageMessageEventContent, MessageType,
    RoomMessageEventContent, SyncRoomMessageEvent, TextMessageEventContent,
};
use matrix_sdk::ruma::events::room::{ImageInfo, MediaSource};
use matrix_sdk::ruma::{MxcUri, OwnedMxcUri, OwnedUserId, ServerName, UserId};
use matrix_sdk::{Client, Room, RoomState};
use reqwest::Url;
use rust_decimal::prelude::*;
use rusty_money::iso::Currency;
use rusty_money::Money;
use tokio::time;
use tokio::time::Duration;

pub async fn get_text_message(
    event: SyncRoomMessageEvent,
    room: Room,
    client: Client,
) -> Option<(Room, OwnedUserId, String)> {
    if room.state() != RoomState::Joined {
        return None;
    }

    let event = event.as_original()?;
    let MessageType::Text(TextMessageEventContent { body, .. }) = &event.content.msgtype else {
        return None;
    };

    if event.sender.as_str() == client.user_id()?.as_str() {
        None
    } else {
        Some((room, event.sender.clone(), body.clone()))
    }
}

pub async fn get_image_message(
    event: SyncRoomMessageEvent,
    room: Room,
    client: Client,
) -> Option<(Room, OwnedUserId, OwnedMxcUri, Box<ImageInfo>)> {
    if room.state() != RoomState::Joined {
        return None;
    }

    let event = event.as_original()?;
    let MessageType::Image(ImageMessageEventContent {
        source: MediaSource::Plain(uri),
        info: Some(info),
        ..
    }) = &event.content.msgtype
    else {
        return None;
    };

    if event.sender.as_str() == client.user_id()?.as_str() {
        None
    } else {
        Some((room, event.sender.clone(), uri.clone(), info.clone()))
    }
}

pub async fn get_file_message(
    event: SyncRoomMessageEvent,
    room: Room,
    client: Client,
) -> Option<(Room, OwnedUserId, OwnedMxcUri, Box<FileInfo>)> {
    if room.state() != RoomState::Joined {
        return None;
    }

    let event = event.as_original()?;
    let MessageType::File(FileMessageEventContent {
        source: MediaSource::Plain(uri),
        info: Some(info),
        ..
    }) = &event.content.msgtype
    else {
        return None;
    };

    if event.sender.as_str() == client.user_id()?.as_str() {
        None
    } else {
        Some((room, event.sender.clone(), uri.clone(), info.clone()))
    }
}

pub fn find_command<'a>(prefixes: Vec<&str>, message: &'a str) -> Option<&'a str> {
    for prefix in &prefixes {
        if let Some(command) = get_command(prefix, message) {
            return Option::Some(command);
        }
    }

    Option::None
}

pub fn get_command<'a>(prefix: &str, message: &'a str) -> Option<&'a str> {
    let lower_message = message.to_lowercase();
    let lower_prefix = prefix.to_lowercase();

    if lower_message.eq(&lower_prefix) {
        return Some("");
    }

    if lower_message.starts_with(&format!("{} ", lower_prefix)) {
        return Some(message[prefix.len() + 1..].trim());
    }

    if lower_message.starts_with(&format!("{}. ", lower_prefix)) {
        return Some(message[prefix.len() + 2..].trim());
    }

    Option::None
}

async fn on_room_invitation(room_member: StrippedRoomMemberEvent, client: Client, room: Room) {
    if room_member.state_key.as_str() != client.user_id().unwrap().as_str() {
        return;
    }

    if room.state() == RoomState::Invited {
        println!("Autojoining room {}", room.room_id());
        let mut delay = 2;

        while let Err(err) = room.join().await {
            // retry autojoin due to synapse sending invites, before the
            // invited user can join for more information see
            // https://github.com/matrix-org/synapse/issues/4345
            eprintln!(
                "Failed to join room {} ({:?}), retrying in {}s",
                room.room_id(),
                err,
                delay
            );

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
    let username = env::var("USERNAME").expect("USERNAME environmental variable not set");

    let password = env::var("PASSWORD").expect("PASSWORD environmental variable not set");

    let homeserver = env::var("HOMESERVER").expect("HOMESERVER environmental variable not set");

    let mut config = dirs::config_dir().expect("no config directory found");
    config.push(bot_name);

    println!("saving configuration to {:?}", config);

    let homeserver_url = Url::parse(&homeserver).expect("invalid homeserver url");
    let client = Client::builder()
        .homeserver_url(homeserver_url.as_str())
        .sqlite_store(config, None)
        .build()
        .await?;

    client
        .matrix_auth()
        .login_username(&username, &password)
        .initial_device_display_name(bot_name)
        .send()
        .await?;

    println!("logged in as {}", username);

    client.sync_once(SyncSettings::default()).await?;
    client.add_event_handler(on_room_invitation);

    Ok(client)
}

pub fn text_plain(message: &str) -> RoomMessageEventContent {
    RoomMessageEventContent::text_plain(message)
}

pub fn text_html(plain: &str, html: &str) -> RoomMessageEventContent {
    RoomMessageEventContent::text_html(plain, html)
}

pub fn normalize_sender(sender: OwnedUserId, command: &str) -> anyhow::Result<OwnedUserId> {
    let sender = if !command.is_empty() {
        create_user_id(command)?
    } else {
        sender
    };

    Ok(sender)
}

pub fn create_user_id(id: &str) -> anyhow::Result<OwnedUserId> {
    let id = id.to_lowercase();
    let id = id.trim();
    let id = id.trim_end_matches(['.', '!', '?']);

    let id = if id == "dad" {
        OwnedUserId::try_from("@phil:kulak.us")?
    } else if id == "mom" {
        OwnedUserId::try_from("@gwen:kulak.us")?
    } else {
        UserId::parse_with_server_name(id, <&ServerName>::try_from("kulak.us")?)?
    };

    Ok(id)
}

pub fn pretty_user_id(user_id: &UserId) -> String {
    match user_id.as_str() {
        "@phil:kulak.us" => return "Dad".to_string(),
        "@gwen:kulak.us" => return "Mom".to_string(),
        _ => (),
    }

    let localpart = &mut user_id.localpart().to_string();

    if localpart.contains('-') {
        return localpart.to_string();
    }

    if let Some(s) = localpart.get_mut(0..1) {
        s.make_ascii_uppercase()
    };

    localpart.to_string()
}

pub fn is_admin(user_id: &UserId) -> bool {
    user_id.as_str().eq_ignore_ascii_case("@phil:kulak.us")
        || user_id.as_str().eq_ignore_ascii_case("@gwen:kulak.us")
}

pub fn money_to_i64(money: &Money<Currency>) -> i64 {
    (*money.amount() * Decimal::from(100)).to_i64().unwrap()
}

pub async fn download_photo(uri: &MxcUri) -> anyhow::Result<Bytes> {
    let id = uri.media_id().unwrap();
    let url = format!("https://kulak.us/_matrix/media/r0/download/kulak.us/{}", id);

    // download the image to memory
    let response = reqwest::Client::new().get(url).send().await?;
    let photo = response.bytes().await?;

    Ok(photo)
}
