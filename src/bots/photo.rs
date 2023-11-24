use std::collections::{HashMap, HashSet};

use std::sync::mpsc;
use std::sync::mpsc::{Receiver, SyncSender};
use std::time::{SystemTime, UNIX_EPOCH};
use std::{env, fs};

use anyhow::bail;
use bytes::Bytes;
use lettre::message::{Attachment, Body, MultiPart};
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::{Client, SyncSettings};
use tokio::task;

use crate::image;
use crate::matrix;
use crate::message_buffer::MessageBuffer;

pub async fn main() -> anyhow::Result<()> {
    let (tx, rx): (SyncSender<MessageEvent>, Receiver<MessageEvent>) = mpsc::sync_channel(1000);
    let client = matrix::create_client("photobot").await?;
    let mut bot = Bot::new();

    client
        .clone()
        .register_event_handler({
            move |event: SyncMessageEvent<MessageEventContent>, room: Room| {
                let tx = tx.clone();
                async move {
                    tx.send(MessageEvent { event, room }).unwrap();
                }
            }
        })
        .await;

    task::spawn({
        let client = client.clone();

        async move {
            let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
            client.sync(settings).await;
        }
    });

    let mut buffer = MessageBuffer::new(&rx);

    loop {
        let message = buffer.poll();
        let room = message.room.clone();

        match bot
            .on_room_message(message.event, message.room, client.clone())
            .await
        {
            Ok(sent) => {
                if sent {
                    buffer.inc()
                }

                let total = buffer.get_final_count();

                if total > 0 {
                    if let Room::Joined(joined) = room {
                        joined
                            .send(matrix::text_plain(&bot.recipients_friendly(total)), None)
                            .await?;
                    }
                }
            }
            Err(err) => {
                if let Room::Joined(joined) = room {
                    joined
                        .send(matrix::text_plain(&err.to_string()), None)
                        .await?;
                } else {
                    print!("could not run message loop: {}", err);
                }
            }
        };
    }
}

struct MessageEvent {
    event: SyncMessageEvent<MessageEventContent>,
    room: Room,
}

struct Bot {
    only: Option<HashMap<String, String>>,
}

impl Bot {
    fn new() -> Bot {
        Bot { only: None }
    }

    async fn on_room_message(
        &mut self,
        event: SyncMessageEvent<MessageEventContent>,
        room: Room,
        client: Client,
    ) -> anyhow::Result<bool> {
        // text messages
        if let Some((joined, _, message)) =
            matrix::get_text_message(event.clone(), room.clone(), client.clone()).await
        {
            // see what's going on
            if matrix::get_command("who", &message).is_some() {
                joined
                    .send(matrix::text_plain(&self.recipients_friendly(0)), None)
                    .await?;

            // reset the recipients
            } else if matrix::get_command("reset", &message).is_some() {
                self.only = None;
                joined
                    .send(matrix::text_plain(&self.recipients_friendly(0)), None)
                    .await?;

            // help!
            } else if matrix::get_command("help", &message).is_some() {
                let text = vec![
                    "who: Show who photos are currently being sent to.",
                    "to mark: Only send photos to Mark.",
                    "to mark jane: Only send photos to Mark and Jane.",
                    "not mark: Don't send photos to Mark.",
                    "reset: Send photos to everyone.",
                ];

                let html = vec![
                    "<ul>",
                    "<li><strong>who</strong>: Show who photos are currently being sent to.</li>",
                    "<li><strong>to mark</strong>: Only send photos to Mark.</li>",
                    "<li><strong>to mark jane</strong>: Only send photos to Mark and Jane.</li>",
                    "<li><strong>not mark</strong>: Don't send photos to Mark.</li>",
                    "<li><strong>reset</strong>: Send photos to everyone.</li>",
                    "</ul>",
                ];

                joined
                    .send(matrix::text_html(&text.join("\n"), &html.join("\n")), None)
                    .await?;

            // skip some recipients
            } else if let Some(command) = matrix::get_command("not", &message) {
                let recipients = self.command_as_recipients(command)?;
                let mut filtered = self.recipients();
                for skip in recipients {
                    filtered.remove(&skip);
                }
                self.only = Some(filtered.clone());

                joined
                    .send(matrix::text_plain(&self.recipients_friendly(0)), None)
                    .await?;

                println!("only sending to {:?}", self.only);

            // only send to some recipients
            } else if let Some(command) =
                matrix::find_command(vec!["to", "send to", "only"], &message)
            {
                let recipients = self.command_as_recipients(command)?;
                let all = Bot::all_recipients();
                let mut filtered: HashMap<String, String> = HashMap::new();
                for to in &recipients {
                    filtered.insert(to.clone(), all[to].clone());
                }
                self.only = Some(filtered.clone());

                joined
                    .send(matrix::text_plain(&self.recipients_friendly(0)), None)
                    .await?;

                println!("only sending to {:?}", self.only);
            }
        }

        // photos
        if let Some((_, _, uri, info)) =
            matrix::get_image_message(event.clone(), room.clone(), client.clone()).await
        {
            println!("got photo mime type of {:#?}", info.mimetype);

            let photo = &matrix::download_photo(&uri).await?;

            let jpeg = match info.mimetype.as_deref() {
                Some("image/heic") | Some("image/heif") => {
                    image::convert_heic_to_jpeg(photo)?
                }
                _ => image::shrink_jpeg(photo)?
            };

            self.send_photo(&jpeg, photo, &info.mimetype.unwrap())
                .await?;

            return Ok(true);
        }

        // files
        if let Some((joined, _, uri, info)) =
            matrix::get_file_message(event.clone(), room.clone(), client.clone()).await
        {
            println!("got mime type of {:#?}", info.mimetype);

            match info.mimetype.as_deref() {
                Some("image/heic") | Some("image/heif") => {
                    let photo = &matrix::download_photo(&uri).await?;
                    let jpeg = image::convert_heic_to_jpeg(photo)?;
                    self.send_photo(&jpeg, photo, &info.mimetype.unwrap())
                        .await?;
                    return Ok(true);
                }
                _ => {
                    joined
                        .send(
                            matrix::text_plain("I don't know what to do with that file. :("),
                            None,
                        )
                        .await?;
                }
            };
        }

        Ok(false)
    }

    async fn send_photo(
        &mut self,
        jpeg: &Bytes,
        photo: &Bytes,
        mime_type: &str,
    ) -> anyhow::Result<()> {
        send_emails(jpeg, "image/jpeg", self.recipients().values())?;
        save_photo(photo, mime_type)?;

        Ok(())
    }

    fn all_recipients() -> HashMap<String, String> {
        let json = env::var("SMTP_TO").expect("SMTP_TO environmental variable not set");
        serde_json::from_str(json.as_str()).unwrap()
    }

    fn recipients(&self) -> HashMap<String, String> {
        match self.only.clone() {
            Some(recipients) => recipients,
            None => Bot::all_recipients(),
        }
    }

    fn command_as_recipients(&self, command: &str) -> anyhow::Result<HashSet<String>> {
        let all = Bot::all_recipients();
        let mut collected: HashSet<String> = HashSet::new();

        for recip in command.split(' ') {
            let r = recip.to_lowercase();

            // allow sending to the Google album only
            if r == "google" {
                return Ok(HashSet::new());
            }

            match all.get(&r) {
                Some(_) => collected.insert(r.to_string()),
                None => bail!("I don't know who {} is!", recip),
            };
        }

        Ok(collected)
    }

    fn recipients_friendly(&self, total: usize) -> String {
        let mut rec: Vec<String> = self.recipients().keys().map(|k| name_case(k)).collect();

        rec.sort();

        let who = match rec.len() {
            0 => "the Google album only".to_string(),
            1 => String::from(rec.first().unwrap()),
            2 => format!("{} and {}", rec[0], rec[1]),
            _ => {
                let head = &rec[0..rec.len() - 2].join(", ");
                let tail = format!("{} and {}", rec[rec.len() - 2], rec[rec.len() - 1]);
                format!("{}, {}", head, tail)
            }
        };

        if total > 0 {
            let label = if total == 1 { "photo" } else { "photos" };
            format!("Sent {} {} to {}.", total, label, who)
        } else {
            format!("Photos will be sent to {}.", who)
        }
    }
}

fn name_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}

fn get_filename(mime_type: &str) -> String {
    let ext = mime_type.split('/').last().unwrap().to_lowercase();

    match ext.as_str() {
        "jpeg" => "photo.jpg".to_string(),
        _ => format!("photo.{}", ext),
    }
}

fn save_photo(photo: &Bytes, mime_type: &str) -> anyhow::Result<()> {
    let ext = mime_type.split('/').last().unwrap();

    let prefix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let dir = env::var("DROPBOX").expect("DROPBOX environmental variable not set");

    let path = format!("{}/{}.{}", dir, prefix, ext);

    Ok(fs::write(path, photo)?)
}

// TODO: this should be async
fn send_emails<'a, I>(photo: &Bytes, mime_type: &str, to: I) -> anyhow::Result<()>
where
    I: Iterator<Item = &'a String>,
{
    let to = Vec::from_iter(to);

    let username = env::var("SMTP_USERNAME").expect("SMTP_USERNAME environmental variable not set");

    let password = env::var("SMTP_PASSWORD").expect("SMTP_PASSWORD environmental variable not set");

    let server = env::var("SMTP_SERVER").expect("SMTP_SERVER environmental variable not set");

    let from = env::var("SMTP_FROM").expect("SMTP_FROM environmental variable not set");

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
            .multipart(MultiPart::mixed().singlepart(
                Attachment::new(get_filename(mime_type)).body(body.clone(), mime_type.parse()?),
            ))?;

        match mailer.send(&email) {
            Ok(_) => println!("Sent photo to {}", address),
            Err(e) => panic!("Could not send email: {:?}", e),
        }
    }

    Ok(())
}
