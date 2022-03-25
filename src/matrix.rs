use matrix_sdk::{room::Room, ruma::events::{room::member::MemberEventContent, StrippedStateEvent}, Client, ClientConfig};
use tokio::time::{sleep, Duration};
use reqwest::Url;

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

            sleep(Duration::from_secs(delay)).await;
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
    let mut home = dirs::cache_dir().expect("no cache directory found");
    home.push(bot_name);

    let client_config = ClientConfig::new().store_path(home);
    let homeserver_url = Url::parse(homeserver_url).expect("invalid homeserver url");
    let client = Client::new_with_config(homeserver_url, client_config).unwrap();

    client.login(username, password, None, Some(bot_name)).await?;

    println!("logged in as {}", username);

    client.register_event_handler(on_room_invitation).await;

    Ok(client)
}
