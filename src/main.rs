extern crate core;

use std::env;

mod webhook;
mod bots;
mod matrix;
mod ai;
mod message_buffer;
mod image;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if let Some(bot) = env::args().nth(1) {
        match bot.as_str() {
            "home" => bots::home::main().await?,
            "money" => bots::money::main().await?,
            "owen" => bots::owen::main().await?,
            "ai" => bots::ai::main().await?,
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
