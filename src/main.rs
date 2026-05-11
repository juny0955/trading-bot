use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info};
use trading_bot::binance_spot_ws::{Ticker, subscribe_to_binance_spot_ws};

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let (tx, mut rx) = mpsc::channel::<Ticker>(100);

    tokio::spawn(async move {
        if let Err(e) = subscribe_to_binance_spot_ws(tx).await {
            error!("WS 연결 에러: {e}");
        }
    });

    while let Some(ticker) = rx.recv().await {
        info!(
            symbol = %ticker.symbol,
            price = %ticker.price,
            event_time = %ticker.event_time,
            "ticker"
        );
    }

    Ok(())
}
