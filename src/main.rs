use anyhow::Result;
use trading_bot::app;

#[tokio::main]
async fn main() -> Result<()> {
    app::run().await
}
