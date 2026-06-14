#![recursion_limit = "512"]

extern crate core;

use std::env;

mod bots;
mod image;
mod matrix;
mod message_buffer;
mod webhook;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(bot) = env::args().nth(1) {
        match bot.as_str() {
            "home" => bots::home::main().await?,
            "money" => bots::money::main().await?,
            "photo" => bots::photo::main().await?,
            _ => {
                println!("unknown bot: {}", bot);
                return Ok(());
            }
        }
    }

    println!("usage: bots {{bot name}}");

    Ok(())
}
