use std::sync::{Arc, Mutex};
use anyhow;
use chrono;
use matrix_sdk::room::Room;
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::SyncSettings;
use rusqlite::{Connection, params};

use crate::matrix;

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("moneybot").await?;
    let bot = Arc::new(Mutex::new(Bot::new()?));

    client.register_event_handler({
        move |event: SyncMessageEvent<MessageEventContent>, room: Room| {
            let bot = bot.clone();
            async move {
                bot.lock().unwrap().on_room_message(event, room);
            }
        }
    }).await;

    let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
    client.sync(settings).await;

    Ok(())
}

struct Transaction {
    sender: Option<String>,
    receiver: String,
    amount: isize,
    date: String,
    memo: Option<String>
}

struct Bot {
    conn: Connection
}

impl Bot {
    fn new() -> anyhow::Result<Bot> {
        let mut db_file = dirs::config_dir().expect("no config directory found");
        db_file.push("moneybot");
        db_file.push("database");

        let db_created = !db_file.exists();

        let database = Bot { conn: Connection::open(db_file)? };

        if db_created {
            database.init()?;
        }

        Ok(database)
    }

    fn init(self: &Bot) -> anyhow::Result<()> {
        self.conn.execute(
            "CREATE TABLE transactions (
            id INTEGER PRIMARY KEY,
            sender TEXT,
            receiver TEXT NOT NULL,
            amount INTEGER NOT NULL,
            date TEXT NOT NULL,
            memo TEXT
        )",
            []
        )?;

        self.conn.execute("CREATE INDEX transaction_senders ON transactions (sender)", [])?;
        self.conn.execute("CREATE INDEX transaction_receivers ON transactions (receiver)", [])?;

        // the two seed transactions
        self.insert(&Transaction {
            sender: None,
            receiver: "gwen@kulak.us".to_string(),
            amount: 1_000_000,
            date: chrono::Utc::now().to_rfc3339(),
            memo: Some("seed value".to_string())
        })?;

        self.insert(&Transaction {
            sender: None,
            receiver: "phil@kulak.us".to_string(),
            amount: 1_000_000,
            date: chrono::Utc::now().to_rfc3339(),
            memo: Some("seed value".to_string())
        })?;

        println!("initialized new database");

        Ok(())
    }

    fn insert(self: &Bot, t: &Transaction) -> anyhow::Result<()> {
        self.conn.execute(
            "INSERT INTO transactions
            (sender, receiver, amount, date, memo)
        VALUES
            (?1, ?2, ?3, ?4, ?5)",
            params![t.sender, t.receiver, t.amount, t.date, t.memo]
        )?;

        Ok(())
    }

    async fn on_room_message(
        self: &Bot,
        event: SyncMessageEvent<MessageEventContent>,
        room: Room
    ) {}
}
