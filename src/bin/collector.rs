use anyhow::Result;
use trading_bot::collector::app;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
