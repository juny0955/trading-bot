use anyhow::Result;
use futures_util::future::join_all;
use tokio::sync::mpsc::{self, Sender};
use tokio::task::JoinHandle;
use tracing::info;
use trading_bot::dtos::StreamData;
use trading_bot::writer::DbEvent;
use trading_bot::{BinanceConfig, BinanceRuntimeConfig, FngRuntimeConfig, SharedConfig, writer};
use trading_bot::{
    alterantive_fng::fetch_alternative_fng,
    binance_futures_ws::subscribe_to_binance_futures_ws,
    db::config_reader::{init_db, load_config},
    dtos::{BookTickerData, DepthData, FngData, TradeData},
};

#[tokio::main]
async fn main() -> Result<()> {
    setup();
    let config = load_config_to_sqlite();

    let questdb_url = std::env::var("QUESTDB_URL").expect("DB URL 없음");
    let (db_tx, db_rx) = mpsc::channel::<DbEvent>(1000);

    let mut handles = Vec::new();
    handles.push(tokio::spawn(async move {
        writer::run(&questdb_url, db_rx).await
    }));
    handles.extend(spawn_fng(config.runtime.fng.clone(), db_tx.clone()));
    handles.extend(spawn_binance(
        config.binance.clone(),
        config.runtime.binance.clone(),
        db_tx,
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

fn load_config_to_sqlite() -> SharedConfig {
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

    config
}

fn spawn_fng(cfg: FngRuntimeConfig, db_tx: Sender<DbEvent>) -> Vec<JoinHandle<()>> {
    let (fng_tx, mut fng_rx) = mpsc::channel::<FngData>(1);
    vec![
        tokio::spawn(async move {
            fetch_alternative_fng(cfg, fng_tx).await;
        }),
        tokio::spawn(async move {
            while let Some(f) = fng_rx.recv().await {
                let _ = db_tx.send(DbEvent::Fng(f)).await;
            }
        }),
    ]
}

fn spawn_binance(
    cfg: BinanceConfig,
    runtime_cfg: BinanceRuntimeConfig,
    db_tx: Sender<DbEvent>,
) -> Vec<JoinHandle<()>> {
    let (stream_tx, mut stream_rx) = mpsc::channel::<StreamData>(100);
    let (trade_tx, mut trade_rx) = mpsc::channel::<TradeData>(100);
    let (depth_tx, mut depth_rx) = mpsc::channel::<DepthData>(100);
    let (book_tx, mut book_rx) = mpsc::channel::<BookTickerData>(100);

    let db_tx_trade = db_tx.clone();
    let db_tx_depth = db_tx.clone();
    let db_tx_book = db_tx;

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
                let _ = db_tx_trade.send(DbEvent::Trade(t)).await;
            }
        }),
        tokio::spawn(async move {
            while let Some(d) = depth_rx.recv().await {
                let _ = db_tx_depth.send(DbEvent::Depth(d)).await;
            }
        }),
        tokio::spawn(async move {
            while let Some(b) = book_rx.recv().await {
                let _ = db_tx_book.send(DbEvent::BookTicker(b)).await;
            }
        }),
    ]
}
