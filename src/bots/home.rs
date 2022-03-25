use std::env;
use matrix_sdk::SyncSettings;
use crate::matrix;

pub async fn main() -> anyhow::Result<()> {
    let username = env::var("USERNAME")
        .expect("USERNAME environmental variable not set");

    let password = env::var("PASSWORD")
        .expect("PASSWORD environmental variable not set");

    let homeserver = env::var("HOMESERVER")
        .expect("HOMESERVER environmental variable not set");

    let client = matrix::create_client("homebot", &homeserver, &username, &password)
        .await?;

    client.sync(SyncSettings::default()).await;

    Ok(())
}
