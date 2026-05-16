use crate::config::{AlternativeRuntimeConfig, BinanceConfig, BinanceRuntimeConfig, SharedConfig};
use crate::market_data::alternative::fng::fetch_alternative_fng;
use crate::market_data::binance::data_ws::subscribe_to_binance_futures_ws;
use crate::market_data::binance::dto::StreamData;
use crate::storage::config_db::{init_db, load_config};
use crate::storage::event::StorageEvent;
use crate::storage::questdb::writer;
use crate::types::FngData;
use anyhow::Result;
use futures_util::future::join_all;
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use tracing::{info, warn};

pub async fn run() -> Result<()> {
    crate::init::setup();
    let config = load_config_to_db().await;
    let quest_db_url = std::env::var("QUESTDB_URL").expect("QuestDB URL 없음");
    let (db_tx, db_rx) = mpsc::channel::<StorageEvent>(1000);
    let token = CancellationToken::new();

    let mut handles = Vec::new();
    let writer_token = token.clone();
    let value = config.clone();
    handles.push(tokio::spawn(async move {
        writer::run(
            &quest_db_url,
            value.runtime.questdb.clone(),
            db_rx,
            writer_token,
        )
        .await
    }));
    handles.extend(spawn_alternative(
        config.runtime.alternative.clone(),
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
        _ = join_all(handles.iter_mut()) => {
            warn!("핸들 조기 종료 - 전체 shutdown");
        }
    }

    token.cancel();
    let _ = join_all(handles).await;
    Ok(())
}

async fn load_config_to_db() -> SharedConfig {
    let pool = init_db().await.expect("POSTGRESQL DB 초기화 실패");

    let app_config = load_config(&pool).await.expect("Config 로드 실패");

    let config = SharedConfig::new(app_config);

    info!(
        "\n================ [ Config Loaded ] ================\n\
     * Symbols  : {:?}\n\
     * Binance  : Reconnect = {}s, Timeout = {}s\n\
     * Alt FNG  : Fallback = {}s, Retry = {}s\n\
     * QuestDB  : Max Rows = {}, Interval = {}ms, Buffer = {}B\n\
     ==================================================",
        config
            .binance
            .symbols
            .iter()
            .filter(|s| s.enabled)
            .map(|s| s.symbol.as_str())
            .collect::<Vec<_>>(),
        config.runtime.binance.reconnect_delay_sec,
        config.runtime.binance.read_timeout_sec,
        config.runtime.alternative.fallback_interval_sec,
        config.runtime.alternative.retry_interval_sec,
        config.runtime.questdb.batch_max_rows,
        config.runtime.questdb.batch_interval_ms,
        config.runtime.questdb.buffer_max_bytes,
    );

    config
}

fn spawn_alternative(
    cfg: AlternativeRuntimeConfig,
    db_tx: Sender<StorageEvent>,
    token: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let (fng_tx, mut fng_rx) = mpsc::channel::<FngData>(1);
    vec![
        tokio::spawn(async move {
            fetch_alternative_fng(cfg, fng_tx, token).await;
        }),
        tokio::spawn(async move {
            while let Some(f) = fng_rx.recv().await {
                if let Err(e) = db_tx.send(f.into()).await {
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
    db_tx: Sender<StorageEvent>,
    token: CancellationToken,
) -> Vec<JoinHandle<()>> {
    let (stream_tx, mut stream_rx) = mpsc::channel::<StreamData>(100);
    let mut handles = Vec::new();

    let public_url = cfg.public_ws_url();
    let rc = runtime_cfg.clone();
    let stx = stream_tx.clone();
    let t = token.clone();
    handles.push(tokio::spawn(async move {
        subscribe_to_binance_futures_ws(public_url, "Public", rc, stx, t).await
    }));

    let market_url = cfg.market_ws_url();
    handles.push(tokio::spawn(async move {
        subscribe_to_binance_futures_ws(market_url, "Market", runtime_cfg, stream_tx, token).await
    }));

    handles.push(tokio::spawn(async move {
        while let Some(s) = stream_rx.recv().await {
            if let Err(e) = db_tx.send(s.into()).await {
                warn!("QuestDB Tx 채널 닫힘: {e}");
                return;
            }
        }
    }));

    handles
}
