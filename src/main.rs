use anyhow::Result;
use tokio::sync::mpsc;
use tracing::{error, info};
use trading_bot::{
    alterantive_fng::fetch_alternative_fng,
    binance_futures_ws::subscribe_to_binance_futures_ws,
    dtos::{BookTickerData, DepthData, TradeData},
};

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();

    let _ = fetch_alternative_fng().await;

    let (trade_tx, mut trade_rx) = mpsc::channel::<TradeData>(100);
    let (depth_tx, mut depth_rx) = mpsc::channel::<DepthData>(100);
    let (book_tx, mut book_rx) = mpsc::channel::<BookTickerData>(100);

    tokio::spawn(async move {
        if let Err(e) = subscribe_to_binance_futures_ws(trade_tx, depth_tx, book_tx).await {
            error!("WS 연결 에러: {e}");
        }
    });

    tokio::spawn(async move {
        while let Some(t) = trade_rx.recv().await {
            info!(symbol = %t.symbol, price = %t.price, qty = %t.quantity, "trade");
        }
    });
    tokio::spawn(async move {
        while let Some(d) = depth_rx.recv().await {
            info!(
                fupd = d.first_update_id,
                lupd = d.last_update_id,
                bids = d.bids.len(),
                asks = d.asks.len(),
                "depth"
            );
        }
    });

    while let Some(b) = book_rx.recv().await {
        info!(bid = %b.bid_price, ask = %b.ask_price, "book_ticker");
    }

    Ok(())
}
