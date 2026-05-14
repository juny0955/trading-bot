use anyhow::Result;
use futures_util::future::join_all;
use tokio::sync::mpsc::{self};
use tokio::task::JoinHandle;
use tracing::info;
use trading_bot::dtos::StreamData;
use trading_bot::{BinanceConfig, BinanceRuntimeConfig, FngRuntimeConfig, SharedConfig};
use trading_bot::{
    alterantive_fng::fetch_alternative_fng,
    binance_futures_ws::subscribe_to_binance_futures_ws,
    db::config_reader::{init_db, load_config},
    dtos::{BookTickerData, DepthData, FngData, TradeData},
};

#[tokio::main]
async fn main() -> Result<()> {
    setup();

    let config = SharedConfig::new(
        init_db()
            .and_then(|conn| load_config(&conn))
            .expect("Config 로드 실패"),
    );

    info!(
        symbols = ?config.binance.symbols.iter().map(|s| s.symbol.as_str()).collect::<Vec<_>>(),
        binance_reconnect_delay_sec = config.runtime.binance.reconnect_delay_sec,
        fng_fallback_sec = config.runtime.fng.fallback_interval_sec,
        fng_retry_sec = config.runtime.fng.retry_interval_sec,
        "Loaded config"
    );

    let mut handles = Vec::new();
    handles.extend(spawn_fng(config.runtime.fng.clone()));
    handles.extend(spawn_binance(
        config.binance.clone(),
        config.runtime.binance.clone(),
    ));

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

fn spawn_fng(cfg: FngRuntimeConfig) -> Vec<JoinHandle<()>> {
    let (fng_tx, mut fng_rx) = mpsc::channel::<FngData>(1);
    vec![
        tokio::spawn(async move {
            fetch_alternative_fng(cfg, fng_tx).await;
        }),
        tokio::spawn(async move {
            while let Some(f) = fng_rx.recv().await {
                info!(value = %f.value, status = ?f.status, "fng");
            }
        }),
    ]
}

fn spawn_binance(cfg: BinanceConfig, runtime_cfg: BinanceRuntimeConfig) -> Vec<JoinHandle<()>> {
    let (stream_tx, mut stream_rx) = mpsc::channel::<StreamData>(100);
    let (trade_tx, mut trade_rx) = mpsc::channel::<TradeData>(100);
    let (depth_tx, mut depth_rx) = mpsc::channel::<DepthData>(100);
    let (book_tx, mut book_rx) = mpsc::channel::<BookTickerData>(100);

    vec![
        tokio::spawn(
            async move { subscribe_to_binance_futures_ws(cfg, runtime_cfg, stream_tx).await },
        ),
        tokio::spawn(async move {
            while let Some(s) = stream_rx.recv().await {
                match s {
                    StreamData::Trade(d) => {
                        let _ = trade_tx.send(d).await;
                    }
                    StreamData::Depth(d) => {
                        let _ = depth_tx.send(d).await;
                    }
                    StreamData::BookTicker(d) => {
                        let _ = book_tx.send(d).await;
                    }
                }
            }
        }),
        tokio::spawn(async move {
            while let Some(t) = trade_rx.recv().await {
                info!(symbol = %t.symbol, price = %t.price, qty = %t.quantity, "trade");
            }
        }),
        tokio::spawn(async move {
            while let Some(d) = depth_rx.recv().await {
                info!(
                    symbol = %d.symbol,
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
