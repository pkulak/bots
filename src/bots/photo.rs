use std::{env, fs};
use std::collections::HashMap;
use std::ops::{Add, Sub};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::{Duration, SystemTime};

use bytes::Bytes;
use lettre::{Message, SmtpTransport, Transport};
use lettre::message::{Attachment, Body, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use matrix_sdk::{Client, SyncSettings};
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::ImageInfo;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::ruma::MxcUri;
use oauth2::{AccessToken, AuthorizationCode, AuthUrl, ClientId, ClientSecret, CsrfToken, RedirectUrl, RefreshToken, Scope, TokenResponse, TokenUrl};
use oauth2::basic::{BasicClient, BasicTokenResponse};
use oauth2::reqwest::async_http_client;
use oauth2::url::Url;
use serde::{Deserialize, Serialize};
use tokio::task;

use crate::matrix;

pub async fn main() -> anyhow::Result<()> {
    let (tx, rx): (SyncSender<MessageEvent>, Receiver<MessageEvent>) = mpsc::sync_channel(10);
    let client = matrix::create_client("photobot").await?;
    let mut bot = Bot::new();

    client.clone().register_event_handler({
        move |event: SyncMessageEvent<MessageEventContent>, room: Room| {
            let tx = tx.clone();
            async move { tx.send(MessageEvent { event, room }).unwrap(); }
        }
    }).await;

    task::spawn({
        let client = client.clone();

        async move {
            let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
            client.sync(settings).await;
        }
    });

    loop {
        let message = rx.recv().unwrap();
        bot.on_room_message(message.event, message.room, client.clone()).await?;
    }
}

struct MessageEvent {
    event: SyncMessageEvent<MessageEventContent>,
    room: Room
}

#[derive(Deserialize, Serialize)]
struct Token {
    access_token: AccessToken,
    refresh_token: RefreshToken,
    expires_at: SystemTime
}

impl Token {
    fn new(resp: BasicTokenResponse, existing: Option<&Token>) -> Token {
        Token {
            access_token: resp.access_token().clone(),
            refresh_token: match resp.refresh_token() {
                Some(token) => token.clone(),
                None => existing.unwrap().refresh_token.clone()
            },
            expires_at: SystemTime::now().add(resp.expires_in().unwrap())
        }
    }

    fn expires_soon(&self) -> bool {
        self.expires_at.sub(Duration::from_secs(300)) < SystemTime::now()
    }
}

struct Bot {
    http: reqwest::Client,
    oauth: BasicClient,
    token: Option<Token>,
    only: Option<HashMap<String, String>>
}

impl Bot {
    fn new() -> Bot {
        let client_id = env::var("CLIENT_ID")
            .expect("CLIENT_ID environmental variable not set");

        let client_secret = env::var("CLIENT_SECRET")
            .expect("CLIENT_SECRET environmental variable not set");

        let client = BasicClient::new(
            ClientId::new(client_id),
            Some(ClientSecret::new(client_secret)),
            AuthUrl::new("https://accounts.google.com/o/oauth2/auth".to_string()).unwrap(),
            Some(TokenUrl::new("https://accounts.google.com/o/oauth2/token".to_string()).unwrap())
        )
            .set_redirect_uri(
                RedirectUrl::new("https://accounts.vevo.com/callback".to_string()).unwrap());

        Bot { oauth: client, http: reqwest::Client::new(), token: Bot::load_token(), only: None }
    }

    fn save_token(&self) -> anyhow::Result<()> {
        let json = serde_json::to_string(self.token.as_ref().unwrap())?;
        fs::write(Bot::make_token_path(), json)?;

        println!("saved new auth token");

        Ok(())
    }

    fn load_token() -> Option<Token> {
        match fs::read_to_string(Bot::make_token_path()) {
            Ok(data) => {
                println!("loaded existing token");
                Some(serde_json::from_str(&data).unwrap())
            },
            Err(_) => None
        }
    }

    fn make_token_path() -> PathBuf {
        let mut path = dirs::config_dir().expect("no config directory found");
        path.push("photobot");
        path.push("token.js");

        path
    }

    async fn check_auth(&mut self) -> anyhow::Result<()> {
        if let Some(token) = self.token.as_ref() {
            if token.expires_soon() {
                let resp = self.oauth
                    .exchange_refresh_token(&token.refresh_token)
                    .add_extra_param("access_type", "offline")
                    .request_async(async_http_client).await?;

                self.token = Some(Token::new(resp, Some(token)));
                self.save_token()?;

                println!("refreshed token");
            }
        }

        Ok(())
    }

    fn make_auth(&self) -> String {
        format!("Bearer {}", self.token.as_ref().unwrap().access_token.secret())
    }

    fn begin_auth(&self) -> Url {
        let (auth_url, _) = self.oauth
            .authorize_url(CsrfToken::new_random)
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/photoslibrary.appendonly".to_string()))
            .add_scope(Scope::new(
                "https://www.googleapis.com/auth/photoslibrary.readonly.appcreateddata".to_string()))
            .url();

        auth_url
    }

    async fn complete_auth(&mut self, code: &str) -> anyhow::Result<()> {
        let resp = self.oauth
            .exchange_code(AuthorizationCode::new(String::from(code)))
            .request_async(async_http_client).await?;

        self.token = Some(Token::new(resp, None));
        self.save_token()?;

        Ok(())
    }

    async fn upload_photo(&self, photo: &Bytes, mime_type: &str) -> anyhow::Result<()> {
        if self.token.is_none() {
            println!("no authorization; skipping upload");
            return Ok(())
        }

        let resp = self.http.post("https://photoslibrary.googleapis.com/v1/uploads")
            .header("Authorization", self.make_auth())
            .header("Content-Type", "application/octet-stream")
            .header("X-Goog-Upload-Content-Type", mime_type)
            .header("X-Goog-Upload-Protocol", "raw")
            .body(photo.to_vec())
            .send()
            .await?;

        let token = resp.text().await?;
        let album_id = env::var("ALBUM_ID").expect("ALBUM_ID environmental variable not set");

        let body = format!("{{
            \"albumId\": \"{}\",
                \"newMediaItems\": [
                    {{
                        \"description\": \"Kulak Family Photo\",
                        \"simpleMediaItem\": {{
                            \"fileName\": \"photo.jpg\",
                            \"uploadToken\": \"{}\"
                        }}
                    }}
                ]
            }}", album_id, token);

        self.http.post("https://photoslibrary.googleapis.com/v1/mediaItems:batchCreate")
            .header("Authorization", self.make_auth())
            .header("Content-Type", "application/json")
            .body(body)
            .send()
            .await?;

        println!("Uploaded photo to Google.");

        Ok(())
    }

    async fn on_room_message(
        &mut self,
        event: SyncMessageEvent<MessageEventContent>,
        room: Room,
        client: Client
    ) -> anyhow::Result<()> {
        // text messages
        if let Some((joined, _, message)) =
                matrix::get_text_message(event.clone(), room.clone(), client.clone()).await {

            // start the auth process
            if let Some(_) = matrix::get_command("auth", &message) {
                let url = self.begin_auth();
                joined.send(matrix::text_plain(url.as_str()), None).await?;

            // see what's going on
            } else if let Some(_) = matrix::get_command("who", &message) {
                joined.send(matrix::text_plain(&self.recipients_friendly()), None).await?;

            // reset the recipients
            } else if let Some(_) = matrix::get_command("reset", &message) {
                self.only = None;
                joined.send(matrix::text_plain(&self.recipients_friendly()), None).await?;

            // skip some recipients
            } else if let Some(command) = matrix::get_command("not", &message) {
                let all = self.recipients();
                let mut filtered = self.recipients();

                for skip in command.split(" ") {
                    let found = match all.get(&skip.to_lowercase()) {
                        Some(_) => {
                            filtered.remove(skip);
                            true
                        }
                        None => {
                            joined.send(matrix::text_plain(
                                &format!("I don't know who {} is!", skip)), None).await?;
                            false
                        }
                    };

                    if !found {
                        return Ok(())
                    }
                }

                self.only = Some(filtered.clone());

                joined.send(matrix::text_plain(&self.recipients_friendly()), None).await?;

                println!("only sending to {:?}", self.only);

            // finish up the auth process
            } else if self.token.is_none() {
                self.complete_auth(&message).await?;
                joined.send(matrix::text_plain("Login successful!"), None).await?;
            }
        }

        // photos
        if let Some((_, _, uri, info)) =
                matrix::get_image_message(event.clone(), room.clone(), client.clone()).await {

            let (photo, mime_type) = download_photo(uri, info).await?;
            let emails = Vec::from_iter(self.recipients().into_values());

            send_emails(&photo, &mime_type, emails).await?;
            self.only = None;

            self.check_auth().await?;
            self.upload_photo(&photo, &mime_type).await?;
        }

        Ok(())
    }

    fn recipients(&self) -> HashMap<String, String> {
        match self.only.clone() {
            Some(recipients) => recipients,
            None => {
                let json = env::var("SMTP_TO").expect("SMTP_TO environmental variable not set");
                serde_json::from_str(json.as_str()).unwrap()
            }
        }
    }

    fn recipients_friendly(&self) -> String {
        let rec: Vec<String> = self.recipients().keys()
            .map(|k| name_case(k))
            .collect();

        let who = match rec.len() {
            0 => "no one",
            1 => String::from(rec.first().unwrap()),
            2 => format!("{} and {}", rec[0], rec[1]),
            _ => {
                let head = &rec[0..rec.len() - 2].join(",");
                let tail = format!("{} and {}", rec[rec.len() - 2], rec[rec.len() - 1]);
                format!("{}, {}", head, tail)
            }
        };

        format!("The next photo will be sent to {}.", who)
    }
}

fn name_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

async fn download_photo(uri: MxcUri, info: Box<ImageInfo>) -> anyhow::Result<(Bytes, String)> {
    let id = uri.as_str().split("/").last().unwrap();
    let url = format!("https://kulak.us/_matrix/media/r0/download/kulak.us/{}", id);

    // download the image to memory
    let response = reqwest::Client::new().get(url).send().await?;
    let photo = response.bytes().await?;

    Ok((photo, String::from(info.mimetype.as_ref().unwrap())))
}

async fn send_emails(photo: &Bytes, mime_type: &str, to: Vec<String>) -> anyhow::Result<()> {
    let username = env::var("SMTP_USERNAME")
        .expect("SMTP_USERNAME environmental variable not set");

    let password = env::var("SMTP_PASSWORD")
        .expect("SMTP_PASSWORD environmental variable not set");

    let server = env::var("SMTP_SERVER")
        .expect("SMTP_SERVER environmental variable not set");

    let from = env::var("SMTP_FROM")
        .expect("SMTP_FROM environmental variable not set");

    let creds = Credentials::new(username, password);
    let body = Body::new(photo.to_vec());

    let mailer = SmtpTransport::relay(&server)
        .unwrap()
        .credentials(creds)
        .build();

    for address in to {
        let email = Message::builder()
            .from(from.parse()?)
            .to(address.parse()?)
            .subject("Photo")
            .multipart(
                MultiPart::mixed()
                    .singlepart(Attachment::new(String::from("photo@kulak.us")).body(
                        body.clone(),
                        mime_type.parse()?))
            )?;

        match mailer.send(&email) {
            Ok(_) => println!("Sent photo to {}", address),
            Err(e) => panic!("Could not send email: {:?}", e)
        }
    }

    Ok(())
}
