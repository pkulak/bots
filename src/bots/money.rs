use std::sync::{Arc, Mutex};

use anyhow;
use chrono;
use futures::executor;
use matrix_sdk::{Client, SyncSettings};
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::ruma::UserId;
use rusqlite::{Connection, params};
use rusty_money::{iso, Money};
use tokio::task;

use crate::matrix;

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("moneybot").await?;
    let bot = Arc::new(Mutex::new(Bot::new()?));

    client.register_event_handler({
        move |event: SyncMessageEvent<MessageEventContent>, room: Room, client: Client| {
            let bot = bot.clone();

            task::spawn_blocking(move || {
                executor::block_on(bot.lock().unwrap().on_room_message(event, room, client))
                    .expect("could not run message handler");
            })
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

        let bot = Bot { conn: Connection::open(db_file)? };

        if db_created {
            bot.init()?;
        }

        Ok(bot)
    }

    fn init(self: &Bot) -> anyhow::Result<()> {
        self.conn.execute("
            CREATE TABLE transactions (
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
            receiver: "@gwen:kulak.us".to_string(),
            amount: 100_000,
            date: chrono::Utc::now().to_rfc3339(),
            memo: Some("seed value".to_string())
        })?;

        self.insert(&Transaction {
            sender: None,
            receiver: "@phil:kulak.us".to_string(),
            amount: 100_000,
            date: chrono::Utc::now().to_rfc3339(),
            memo: Some("seed value".to_string())
        })?;

        println!("initialized new database");

        Ok(())
    }

    fn insert(self: &Bot, t: &Transaction) -> anyhow::Result<()> {
        self.conn.execute("
            INSERT INTO transactions
                (sender, receiver, amount, date, memo)
            VALUES
                (?1, ?2, ?3, ?4, ?5)",
            params![t.sender, t.receiver, t.amount, t.date, t.memo]
        )?;

        Ok(())
    }

    fn get_balance(self: &Bot, user_id: &str) -> anyhow::Result<i64> {
        let mut stmt = self.conn
            .prepare("
                SELECT COALESCE(SUM(amount), 0)
                FROM transactions
                WHERE receiver = ?1
            ")
            .unwrap();

        let balance: i64 = stmt
            .query_row(params![user_id], |row| row.get(0))
            .unwrap();

        Ok(balance)
    }

    async fn on_room_message(
        self: &Bot,
        event: SyncMessageEvent<MessageEventContent>,
        room: Room,
        client: Client
    ) -> anyhow::Result<()> {
        if let Some((room, sender, message)) = matrix::get_text_message(event, room, client).await {
            if let Some(command) = matrix::get_command("balance", &message).await {
                self.on_balance_message(room, sender, command).await?;
            }
        }

        Ok(())
    }

    async fn on_balance_message(
        self: &Bot,
        room: Joined,
        sender: UserId,
        command: &str
    ) -> anyhow::Result<()> {
        let sender = matrix::normalize_user_id(sender, command)?;
        let balance = Money::from_minor(self.get_balance(sender.as_str())?, iso::USD);
        room.send(matrix::text_plain(&format!("{}", balance)), None).await?;
        Ok(())
    }
}
