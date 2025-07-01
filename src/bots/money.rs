use std::env;
use std::str::FromStr;
use std::sync::{Arc, Mutex};

use anyhow;
use chrono::{DateTime, Datelike, Duration, TimeZone, Timelike, Utc};
use chrono_tz::US::Pacific;
use futures::executor;
use matrix_sdk::room::{Joined, Room};
use matrix_sdk::ruma::events::room::message::MessageEventContent;
use matrix_sdk::ruma::events::SyncMessageEvent;
use matrix_sdk::ruma::{RoomId, UserId};
use matrix_sdk::{Client, SyncSettings};
use rusqlite::{params, Connection};
use rust_decimal::prelude::ToPrimitive;
use rusty_money::iso::Currency;
use rusty_money::{iso, Money};
use string_builder::Builder;
use tokio::task;

use matrix::text_plain;

use crate::matrix;
use crate::matrix::text_html;

const MAIN_ROOM: &str = "!hMPITSQBLFEleSJmVm:kulak.us";
const SAVINGS: [&str; 1] = ["@charlie-savings@kulak.us"];

pub async fn main() -> anyhow::Result<()> {
    let client = matrix::create_client("moneybot").await?;
    let bot = Arc::new(Mutex::new(Bot::new()?));

    client
        .register_event_handler({
            let bot = bot.clone();

            move |event: SyncMessageEvent<MessageEventContent>, room: Room, client: Client| {
                let bot = bot.clone();

                task::spawn_blocking(move || {
                    executor::block_on(bot.lock().unwrap().on_room_message(event, room, client))
                        .expect("could not run message handler");
                })
            }
        })
        .await;

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

    let chase: i64 = env::var("CHASE")
        .expect("CHASE environmental variable not set")
        .parse()
        .expect("not an integer");

    let charlie: i64 = env::var("CHARLIE")
        .expect("CHARLIE environmental variable not set")
        .parse()
        .expect("not an integer");

    let morning = now
        .with_hour(9)
        .unwrap()
        .with_minute(0)
        .unwrap()
        .with_second(0)
        .unwrap();

    let next_friday =
        morning + Duration::days(5 - morning.weekday().number_from_monday().to_i64().unwrap());

    let next_friday = if next_friday < now {
        next_friday + Duration::days(7)
    } else {
        next_friday
    };

    let duration = next_friday.signed_duration_since(now);
    println!("allowance due in {:?} minutes", duration.num_minutes());

    tokio::time::sleep(duration.to_std().unwrap()).await;

    let room_id = RoomId::try_from(MAIN_ROOM)?;

    {
        let bot = bot.lock().unwrap();
        bot.send(
            "@phil:kulak.us",
            "@chase:kulak.us",
            chase,
            Some("allowance"),
        )?;
        bot.send(
            "@phil:kulak.us",
            "@charlie:kulak.us",
            charlie,
            Some("allowance"),
        )?;
    }

    client
        .room_send(
            &room_id,
            text_plain(
                format!(
                    "Sent {} to Chase and {} to Charlie.",
                    Money::from_minor(chase, iso::USD),
                    Money::from_minor(charlie, iso::USD)
                )
                .as_str(),
            ),
            None,
        )
        .await?;

    // sleep for a tad just to make sure we cycle over
    tokio::time::sleep(Duration::minutes(1).to_std().unwrap()).await;

    Ok(())
}

#[derive(Clone)]
struct Transaction {
    sender: Option<String>,
    receiver: String,
    amount: i64,
    date: String,
    memo: Option<String>,
}

#[derive(Clone)]
struct BalanceTransaction<'a> {
    balance: Money<'a, Currency>,
    user: Option<UserId>,
    amount: Money<'a, Currency>,
    date: DateTime<chrono_tz::Tz>,
    memo: Option<String>,
}

struct Bot {
    conn: Connection,
}

impl Bot {
    fn new() -> anyhow::Result<Bot> {
        let mut db_file = dirs::config_dir().expect("no config directory found");
        db_file.push("moneybot");
        db_file.push("database");

        let db_created = !db_file.exists();

        let bot = Bot {
            conn: Connection::open(db_file)?,
        };

        if db_created {
            bot.init()?;
        }

        Ok(bot)
    }

    fn init(self: &Bot) -> anyhow::Result<()> {
        self.conn.execute(
            "
            CREATE TABLE transactions (
                id INTEGER PRIMARY KEY,
                sender TEXT,
                receiver TEXT NOT NULL,
                amount INTEGER NOT NULL,
                date TEXT NOT NULL,
                memo TEXT
            )",
            [],
        )?;

        self.conn.execute(
            "CREATE INDEX transaction_senders ON transactions (sender)",
            [],
        )?;
        self.conn.execute(
            "CREATE INDEX transaction_receivers ON transactions (receiver)",
            [],
        )?;

        self.conn.execute(
            "
            CREATE TABLE users (
                user_id TEXT PRIMARY KEY,
                min_balance INTEGER NOT NULL
            )",
            [],
        )?;

        let now = chrono::Utc::now().to_rfc3339();

        // the two seed transactions
        self.insert(&Transaction {
            sender: None,
            receiver: "@gwen:kulak.us".to_string(),
            amount: 100_000,
            date: now.to_string(),
            memo: Some("seed value".to_string()),
        })?;

        self.insert(&Transaction {
            sender: None,
            receiver: "@phil:kulak.us".to_string(),
            amount: 100_000,
            date: now,
            memo: Some("seed value".to_string()),
        })?;

        println!("initialized new database");

        Ok(())
    }

    pub fn send(
        self: &Bot,
        from: &str,
        to: &str,
        amount: i64,
        memo: Option<&str>,
    ) -> anyhow::Result<()> {
        self.insert(&Transaction {
            sender: Some(from.to_string()),
            receiver: to.to_string(),
            amount,
            date: chrono::Utc::now().to_rfc3339(),
            memo: memo.map(|s| s.to_string()),
        })?;

        Ok(())
    }

    fn insert(self: &Bot, t: &Transaction) -> anyhow::Result<()> {
        self.conn.execute(
            "
            INSERT INTO transactions
                (sender, receiver, amount, date, memo)
            VALUES
                (?1, ?2, ?3, ?4, ?5)",
            params![t.sender, t.receiver, t.amount, t.date, t.memo],
        )?;

        Ok(())
    }

    fn get_balance(self: &Bot, user_id: &UserId) -> anyhow::Result<Money<Currency>> {
        let mut stmt = self.conn.prepare(
            "
                SELECT COALESCE(SUM(amount), 0)
                FROM transactions
                WHERE sender = ?1
            ",
        )?;

        let sent: i64 = stmt.query_row(params![user_id.as_str()], |row| row.get(0))?;

        let mut stmt = self.conn.prepare(
            "
                SELECT COALESCE(SUM(amount), 0)
                FROM transactions
                WHERE receiver = ?1
            ",
        )?;

        let received: i64 = stmt.query_row(params![user_id.as_str()], |row| row.get(0))?;

        Ok(Money::from_minor(received - sent, iso::USD))
    }

    fn get_min_balance(self: &Bot, user_id: &UserId) -> rusqlite::Result<Money<Currency>> {
        let mut stmt = self.conn.prepare(
            "
                SELECT COALESCE(SUM(min_balance), 0)
                FROM users
                WHERE user_id = ?1
            ",
        )?;

        let min: i64 = stmt.query_row(params![user_id.as_str()], |row| row.get(0))?;
        Ok(Money::from_minor(min, iso::USD))
    }

    fn get_ledger(self: &Bot, user_id: &UserId) -> anyhow::Result<Vec<Transaction>> {
        let mut stmt = self.conn.prepare(
            "
                SELECT *
                FROM transactions
                WHERE receiver = ?1 OR sender = ?1
                ORDER BY date DESC LIMIT 5
            ",
        )?;

        let res = stmt.query_map(params![user_id.as_str()], |row| {
            Ok(Transaction {
                sender: row.get("sender")?,
                receiver: row.get("receiver")?,
                amount: row.get("amount")?,
                date: row.get("date")?,
                memo: row.get("memo")?,
            })
        })?;

        Ok(res.into_iter().map(|row| row.unwrap()).collect())
    }

    fn set_min_balance(self: &Bot, user_id: &UserId, min_balance: i64) -> anyhow::Result<()> {
        self.conn.execute(
            "
            INSERT INTO users
                (user_id, min_balance)
            VALUES
                (?1, ?2)
            ON CONFLICT(user_id) DO UPDATE SET min_balance=?2",
            params![user_id.as_str(), min_balance],
        )?;

        Ok(())
    }

    fn id_exists(self: &Bot, user_id: &UserId) -> anyhow::Result<bool> {
        if SAVINGS.contains(&user_id.as_str()) {
            return Ok(true);
        }

        let mut stmt = self
            .conn
            .prepare(
                "
                SELECT COUNT(*)
                FROM transactions
                WHERE sender = ?1 OR receiver = ?1
            ",
            )
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
        client: Client,
    ) -> anyhow::Result<()> {
        if let Some((room, sender, message)) = matrix::get_text_message(event, room, client).await {
            if let Some(command) = matrix::get_command("balance", &message) {
                self.on_balance_message(room, sender, command).await?;
            } else if let Some(command) = matrix::get_command("send", &message) {
                self.on_send_message(room, sender, command).await?;
            } else if let Some(command) = matrix::get_command("set min", &message) {
                self.on_set_min_balance_message(room, sender, command)
                    .await?;
            } else if let Some(command) = matrix::get_command("get min", &message) {
                self.on_get_min_balance_message(room, command).await?;
            } else if let Some(command) = matrix::get_command("ledger", &message) {
                self.on_ledger_message(room, sender, command).await?;
            }
        }

        Ok(())
    }

    async fn on_balance_message(
        self: &Bot,
        room: Joined,
        sender: UserId,
        command: &str,
    ) -> anyhow::Result<()> {
        let sender = matrix::normalize_sender(sender, command)?;
        let balance = self.get_balance(&sender)?;
        room.send(text_plain(&format!("{}", balance)), None).await?;
        Ok(())
    }

    async fn on_send_message(
        self: &Bot,
        room: Joined,
        sender: UserId,
        command: &str,
    ) -> anyhow::Result<()> {
        let args: Vec<&str> = command
            .split(' ')
            .filter(|w| !w.eq_ignore_ascii_case("to"))
            .filter(|w| !w.trim().is_empty())
            .collect();

        if args.len() < 2 {
            println!("invalid send command {}", command);
            return Ok(());
        }

        let (receiver, amount) = if let Ok(amount) = Money::from_str(args[0], iso::USD) {
            (matrix::create_user_id(args[1])?, amount)
        } else if let Ok(amount) = Money::from_str(args[1], iso::USD) {
            (matrix::create_user_id(args[0])?, amount)
        } else {
            room.send(text_plain("Please use a valid amount."), None)
                .await?;
            return Ok(());
        };

        if amount.is_negative() && !matrix::is_admin(&sender) {
            room.send(
                text_plain("You are not allowed to take money, only send it."),
                None,
            )
            .await?;
            return Ok(());
        }

        if amount.is_zero() {
            room.send(text_plain("Wait... what's the point of that?"), None)
                .await?;
            return Ok(());
        }

        if (self.get_balance(&sender)? - amount.clone()) < self.get_min_balance(&sender)? {
            room.send(text_plain("You don't have enough money!"), None)
                .await?;
            return Ok(());
        }

        if !self.id_exists(&receiver)? && !matrix::is_admin(&sender) {
            room.send(
                text_plain(&format!("{} isn't a valid user.", receiver.localpart())),
                None,
            )
            .await?;
            return Ok(());
        }

        if sender == receiver {
            room.send(
                text_plain("So... you want to send money to yourself, from yourself?"),
                None,
            )
            .await?;
            return Ok(());
        }

        let memo = command.split(" for ").nth(1).map(|s| s.to_string());

        self.insert(&Transaction {
            sender: Some(sender.to_string()),
            receiver: receiver.to_string(),
            amount: matrix::money_to_i64(&amount),
            date: chrono::Utc::now().to_rfc3339(),
            memo: memo.clone(),
        })?;

        let pretty_id = matrix::pretty_user_id(&receiver);

        if memo.is_some() {
            room.send(
                text_plain(&format!(
                    "Sent {} to {} for {}.",
                    amount,
                    pretty_id,
                    memo.unwrap()
                )),
                None,
            )
            .await?;
        } else {
            room.send(
                text_plain(&format!("Sent {} to {}.", amount, pretty_id)),
                None,
            )
            .await?;
        };

        Ok(())
    }

    async fn on_set_min_balance_message(
        self: &Bot,
        room: Joined,
        sender: UserId,
        command: &str,
    ) -> anyhow::Result<()> {
        if !matrix::is_admin(&sender) {
            room.send(
                text_plain("You are not allowed to set minimum balances."),
                None,
            )
            .await?;
            return Ok(());
        }

        let args: Vec<&str> = command.split(' ').collect();

        if args.len() != 2 {
            room.send(text_plain("Usage: set min [user] [amount]."), None)
                .await?;
            return Ok(());
        }

        let user_id = matrix::create_user_id(args[0])?;

        let amount = match Money::from_str(args[1], iso::USD) {
            Ok(amount) => amount,
            Err(_) => {
                room.send(text_plain(&format!("Invalid amount: {}", args[1])), None)
                    .await?;
                return Ok(());
            }
        };

        self.set_min_balance(&user_id, matrix::money_to_i64(&amount))?;

        room.send(
            text_plain(&format!(
                "Set minimum balance for {} to {}",
                matrix::pretty_user_id(&user_id),
                amount
            )),
            None,
        )
        .await?;

        Ok(())
    }

    async fn on_get_min_balance_message(
        self: &Bot,
        room: Joined,
        command: &str,
    ) -> anyhow::Result<()> {
        let args: Vec<&str> = command.split(' ').collect();

        if args.len() != 1 {
            room.send(text_plain("Usage: get min [user]."), None)
                .await?;
            return Ok(());
        }

        let user_id = matrix::create_user_id(args[0])?;
        let min = self.get_min_balance(&user_id)?;

        room.send(text_plain(&format!("{}", min)), None).await?;

        Ok(())
    }

    async fn on_ledger_message(
        self: &Bot,
        room: Joined,
        sender: UserId,
        command: &str,
    ) -> anyhow::Result<()> {
        let user_id = {
            let user_id = command.split(' ').next().unwrap();

            if user_id.to_lowercase() == "plain" {
                sender
            } else {
                matrix::normalize_sender(sender, user_id)?
            }
        };

        let running_balance = &mut self.get_balance(&user_id)?;

        // grab our ledger and convert to balance entries
        let ledger: Vec<BalanceTransaction> = self
            .get_ledger(&user_id)?
            .into_iter()
            .map(|tr| {
                let (user, amount) = if tr.receiver == user_id.as_str() {
                    // I'm the receiver
                    (tr.sender, tr.amount)
                } else {
                    // If I'm the sender, it's a loss; swap the sign
                    (Some(tr.receiver), -tr.amount)
                };

                let transaction = BalanceTransaction {
                    balance: running_balance.clone(),
                    user: user.map(|l| matrix::create_user_id(&l).unwrap()),
                    amount: Money::from_minor(amount, iso::USD),
                    date: Pacific.timestamp_millis(
                        DateTime::<Utc>::from_str(&tr.date)
                            .unwrap()
                            .timestamp_millis(),
                    ),
                    memo: tr.memo,
                };

                *running_balance = Money::from_decimal(
                    running_balance.amount() - transaction.amount.amount(),
                    iso::USD,
                );

                transaction
            })
            .collect();

        // build up our HTML
        let mut html_builder = Builder::default();

        html_builder.append("<table>");
        html_builder.append(
            "<tr><th>Balance</th><th>Amount</th><th>To/From</th><th>For</th><th>Date</th></tr>",
        );

        for tr in ledger.clone() {
            html_builder.append(format!(
                "<tr><td>{}<td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                tr.balance,
                tr.amount,
                tr.user
                    .map(|u| matrix::pretty_user_id(&u))
                    .unwrap_or_default(),
                tr.memo.unwrap_or_default(),
                tr.date.format("%b %d")
            ));
        }

        let pacific_time = Pacific.ymd(1990, 5, 6).and_hms(12, 30, 45);
        pacific_time.with_timezone(&Pacific);

        html_builder.append("</table>");

        // and our text
        let mut txt_builder = Builder::default();

        for tr in ledger {
            let memo = if let Some(memo) = tr.memo {
                format!(" for {}", memo)
            } else {
                "".to_string()
            };

            if tr.amount.is_negative() {
                txt_builder.append(format!(
                    "On {} you sent {} {}{}.",
                    tr.date.format("%b %d"),
                    tr.user.map(|u| matrix::pretty_user_id(&u)).unwrap(),
                    tr.amount * -1,
                    memo
                ));
            } else {
                txt_builder.append(format!(
                    "On {} {} sent you {}{}.",
                    tr.date.format("%b %d"),
                    tr.user.map(|u| matrix::pretty_user_id(&u)).unwrap(),
                    tr.amount,
                    memo
                ));
            }
            txt_builder.append("\n");
        }

        if command.to_lowercase().contains("plain") {
            room.send(text_plain(&txt_builder.string().unwrap()), None)
                .await?;
        } else {
            room.send(
                text_html(
                    &txt_builder.string().unwrap(),
                    &html_builder.string().unwrap(),
                ),
                None,
            )
            .await?;
        }

        Ok(())
    }
}
