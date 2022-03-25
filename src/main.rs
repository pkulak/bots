mod webhook;
mod bots;
mod matrix;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    bots::home::main().await?;

    Ok(())
}
