use std::sync::{Arc, Mutex};

use anyhow;
use chrono::{Datelike, Duration, Local, TimeZone};
use chrono_tz::US::Pacific;
use futures::executor;
use matrix_sdk::{Client, SyncSettings};
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::ruma::{RoomId, UserId};
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use rusqlite::{Connection, params};
use rust_decimal::prelude::ToPrimitive;
use rusty_money::{iso, Money};
use tokio::task;

use matrix::text_plain;

use crate::matrix;

const MAIN_ROOM: &str = "!hMPITSQBLFEleSJmVm:kulak.us";

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("moneybot").await?;
    let bot = Arc::new(Mutex::new(Bot::new()?));

    client.register_event_handler({
        let bot = bot.clone();

        move |event: SyncMessageEvent<MessageEventContent>, room: Room, client: Client| {
            let bot = bot.clone();

            task::spawn_blocking(move || {
                executor::block_on(bot.lock().unwrap().on_room_message(event, room, client))
                    .expect("could not run message handler");
            })
        }
    }).await;

    // manage weekly allowance
    task::spawn({
        let client = client.clone();
        let bot = bot.clone();

        async move {
            loop {
                if let Err(e) = manage_allowance(&client, &bot).await {
                    println!("Could not send allowance! {}", e);
                }
            }
        }
    });

    let settings = SyncSettings::default().token(client.sync_token().await.unwrap());
    client.sync(settings).await;

    Ok(())
}

async fn manage_allowance(client: &Client, bot: &Arc<Mutex<Bot>>) -> anyhow::Result<()> {
    let now = Pacific.timestamp_millis(chrono::Utc::now().timestamp_millis());

    let next_friday = now + Duration::days(
        5 - now.weekday().number_from_monday().to_i64().unwrap());

    let next_friday = if next_friday < now {
        next_friday + Duration::days(7)
    } else {
        next_friday
    };

    let duration = next_friday.signed_duration_since(Local::now());
    println!("allowance due in {:?} days", duration.num_days());

    tokio::time::sleep(duration.to_std().unwrap()).await;

    let room_id = RoomId::try_from(MAIN_ROOM)?;

    {
        let bot = bot.lock().unwrap();
        bot.send("@phil:kulak.us", "@chase:kulak.us", 500, Some("allowance"))?;
        bot.send("@phil:kulak.us", "@chase:kulak.us", 500, Some("allowance"))?;
    }

    client.room_send(&room_id, text_plain("Sent $5 allowance to Chase and Charlie."), None)
        .await?;

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

        let now = chrono::Utc::now().to_rfc3339();

        // the two seed transactions
        self.insert(&Transaction {
            sender: None,
            receiver: "@gwen:kulak.us".to_string(),
            amount: 100_000,
            date: now.to_string(),
            memo: Some("seed value".to_string())
        })?;

        self.insert(&Transaction {
            sender: None,
            receiver: "@phil:kulak.us".to_string(),
            amount: 100_000,
            date: now.to_string(),
            memo: Some("seed value".to_string())
        })?;

        println!("initialized new database");

        Ok(())
    }

    pub fn send(
        self: &Bot,
        from: &str,
        to: &str,
        amount: isize,
        memo: Option<&str>
    ) -> anyhow::Result<()> {
        self.insert(&Transaction {
            sender: Some(from.to_string()),
            receiver: to.to_string(),
            amount,
            date: chrono::Utc::now().to_rfc3339().to_string(),
            memo: memo.map(|s| s.to_string())
        })?;

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

    fn get_balance(self: &Bot, user_id: &UserId) -> anyhow::Result<i64> {
        let mut stmt = self.conn
            .prepare("
                SELECT COALESCE(SUM(amount), 0)
                FROM transactions
                WHERE sender = ?1
            ")
            .unwrap();

        let sent: i64 = stmt
            .query_row(params![user_id.as_str()], |row| row.get(0))
            .unwrap();

        let mut stmt = self.conn
            .prepare("
                SELECT COALESCE(SUM(amount), 0)
                FROM transactions
                WHERE receiver = ?1
            ")
            .unwrap();

        let received: i64 = stmt
            .query_row(params![user_id.as_str()], |row| row.get(0))
            .unwrap();

        Ok(received - sent)
    }

    fn id_exists(self: &Bot, user_id: &UserId) -> anyhow::Result<bool> {
        let mut stmt = self.conn
            .prepare("
                SELECT COUNT(*)
                FROM transactions
                WHERE sender = ?1 OR receiver = ?1
            ")
            .unwrap();

        let total: i64 = stmt
            .query_row(params![user_id.as_str()], |row| row.get(0))
            .unwrap();

        Ok(total > 0)
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
            } else if let Some(command) = matrix::get_command("send", &message).await {
                self.on_send_message(room, sender, command).await?;
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
        let sender = matrix::normalize_sender(sender, command)?;
        let balance = Money::from_minor(self.get_balance(&sender)?, iso::USD);
        room.send(text_plain(&format!("{}", balance)), None).await?;
        Ok(())
    }

    async fn on_send_message(
        self: &Bot,
        room: Joined,
        sender: UserId,
        command: &str
    ) -> anyhow::Result<()> {
        let args: Vec<&str> = command.split(" ")
            .filter(|w| !w.eq_ignore_ascii_case("to"))
            .filter(|w| w.trim().len() > 0)
            .collect();

        if args.len() < 2 {
            println!("invalid send command {}", command);
            return Ok(())
        }

        let (receiver, amount) = if let Ok(amount) = Money::from_str(args[0], iso::USD) {
            (matrix::create_user_id(args[1])?, amount)
        } else if let Ok(amount) = Money::from_str(args[1], iso::USD) {
            (matrix::create_user_id(args[0])?, amount)
        } else {
            room.send(text_plain("Please use a valid amount."), None).await?;
            return Ok(())
        };

        if amount.is_negative() && !matrix::is_admin(&sender) {
            room.send(text_plain(
                "You are not allowed to take money, only send it."), None).await?;
            return Ok(())
        }

        if amount.is_zero() {
            room.send(text_plain("Wait... what's the point of that?"), None).await?;
            return Ok(())
        }

        if self.get_balance(&sender)? < 0 {
            room.send(text_plain("You don't have enough money!"), None).await?;
            return Ok(())
        }

        if !self.id_exists(&receiver)? && !matrix::is_admin(&sender) {
            room.send(text_plain(
                &format!("{} isn't a valid user.", receiver.localpart())), None).await?;
            return Ok(())
        }

        if sender == receiver {
            room.send(text_plain(
                "So... you want to send money to yourself, from yourself?"), None).await?;
            return Ok(())
        }

        let memo = command.split(" for ").nth(1).map(|s| s.to_string());

        self.insert(&Transaction {
            sender: Some(sender.to_string()),
            receiver: receiver.to_string(),
            amount: (amount.amount().to_i64().unwrap() * 100).to_isize().unwrap(),
            date: chrono::Utc::now().to_rfc3339(),
            memo: memo.clone()
        })?;

        let pretty_id = matrix::pretty_user_id(&receiver);

        if memo.is_some() {
            room.send(text_plain(
                &format!("Sent {} to {} for {}.", amount, pretty_id, memo.unwrap())), None).await?;
        } else {
            room.send(text_plain(
                &format!("Sent {} to {}.", amount, pretty_id)), None).await?;
        };

        Ok(())
    }
}
