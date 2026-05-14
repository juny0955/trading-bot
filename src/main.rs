use anyhow::Result;
use futures_util::future::join_all;
use tokio::sync::mpsc::{self, Sender};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};
use trading_bot::dtos::StreamData;
use trading_bot::writer::DbEvent;
use trading_bot::{BinanceConfig, BinanceRuntimeConfig, FngRuntimeConfig, SharedConfig, writer};
use trading_bot::{
    alternative_fng::fetch_alternative_fng,
    binance_futures_ws::subscribe_to_binance_futures_ws,
    db::config_reader::{init_db, load_config},
    dtos::FngData,
};

#[tokio::main]
async fn main() -> Result<()> {
    setup();
    let config = load_config_to_sqlite();
    let quest_db_url = std::env::var("QUESTDB_URL").expect("DB URL 없음");
    let (db_tx, db_rx) = mpsc::channel::<DbEvent>(1000);
    let token = CancellationToken::new();

    let mut handles = Vec::new();
    let writer_token = token.clone();
    handles.push(tokio::spawn(async move {
        writer::run(&quest_db_url, db_rx, writer_token).await
    }));
    handles.extend(spawn_fng(
        config.runtime.fng.clone(),
        db_tx.clone(),
        token.clone(),
    ));
    handles.extend(spawn_binance(
        config.binance.clone(),
        config.runtime.binance.clone(),
        db_tx,
        token.clone(),
    ));

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {
            info!("Ctrl+C graceful shutdown 시작");
            token.cancel();
        }
        _ = join_all(handles.iter_mut()) => {}
    }

    let _ = join_all(handles).await;
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

fn spawn_fng(
    cfg: FngRuntimeConfig,
    db_tx: Sender<DbEvent>,
    token: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let (fng_tx, mut fng_rx) = mpsc::channel::<FngData>(1);
    vec![
        tokio::spawn(async move {
            fetch_alternative_fng(cfg, fng_tx, token).await;
        }),
        tokio::spawn(async move {
            while let Some(f) = fng_rx.recv().await {
                if let Err(e) = db_tx.send(DbEvent::Fng(f)).await {
                    warn!("QuestDB Tx 채널 닫힘: {e}");
                    return;
                }
            }
        }),
    ]
}

fn spawn_binance(
    cfg: BinanceConfig,
    runtime_cfg: BinanceRuntimeConfig,
    db_tx: Sender<DbEvent>,
    token: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let (stream_tx, mut stream_rx) = mpsc::channel::<StreamData>(100);

    vec![
        tokio::spawn(async move {
            subscribe_to_binance_futures_ws(cfg, runtime_cfg, stream_tx, token).await
        }),
        tokio::spawn(async move {
            while let Some(s) = stream_rx.recv().await {
                let event = match s {
                    StreamData::Trade(d) => DbEvent::Trade(d),
                    StreamData::Depth(d) => DbEvent::Depth(d),
                    StreamData::BookTicker(d) => DbEvent::BookTicker(d),
                };

                if let Err(e) = db_tx.send(event).await {
                    warn!("QuestDB Tx 채널 닫힘: {e}");
                    return;
                }
            }
        }),
    ]
}
