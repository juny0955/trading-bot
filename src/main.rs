use anyhow::Result;
use futures_util::future::join_all;
use tokio::sync::mpsc::{self};
use tokio::task::JoinHandle;
use tracing::info;
use trading_bot::{
    alterantive_fng::fetch_alternative_fng,
    binance_futures_ws::subscribe_to_binance_futures_ws,
    dtos::{BookTickerData, DepthData, FngData, TradeData},
};

#[tokio::main]
async fn main() -> Result<()> {
    setup();

    let mut handles = Vec::new();
    handles.extend(spawn_fng());
    handles.extend(spawn_binance());

    join_all(handles).await;
    Ok(())
}

fn setup() {
    rustls::crypto::aws_lc_rs::default_provider()
        .install_default()
        .expect("Failed to install crypto provider");
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .init();
}

fn spawn_fng() -> Vec<JoinHandle<()>> {
    let (fng_tx, mut fng_rx) = mpsc::channel::<FngData>(1);

    vec![
        tokio::spawn(async move {
            fetch_alternative_fng(fng_tx).await;
        }),
        tokio::spawn(async move {
            while let Some(f) = fng_rx.recv().await {
                info!(value = %f.value, status = ?f.status, "fng");
            }
        }),
    ]
}

fn spawn_binance() -> Vec<JoinHandle<()>> {
    let (trade_tx, mut trade_rx) = mpsc::channel::<TradeData>(100);
    let (depth_tx, mut depth_rx) = mpsc::channel::<DepthData>(100);
    let (book_tx, mut book_rx) = mpsc::channel::<BookTickerData>(100);

    vec![
        tokio::spawn(
            async move { subscribe_to_binance_futures_ws(trade_tx, depth_tx, book_tx).await },
        ),
        tokio::spawn(async move {
            while let Some(t) = trade_rx.recv().await {
                info!(symbol = %t.symbol, price = %t.price, qty = %t.quantity, "trade");
            }
        }),
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
        }),
        tokio::spawn(async move {
            while let Some(b) = book_rx.recv().await {
                info!(bid = %b.bid_price, ask = %b.ask_price, "book_ticker");
            }
        }),
    ]
}
