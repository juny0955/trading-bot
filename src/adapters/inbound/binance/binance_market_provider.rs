use std::time::Duration;

use crate::adapters::binance::ws::reconnect::{
    ReconnectConfig, WsError, WsSession, run_with_reconnect,
};
use crate::adapters::inbound::binance::dto::{BinanceStreamData, BinanceStreamEnvelope};
use crate::config::BinanceRuntimeConfig;
use crate::config::binance_config::BinanceConfig;
use crate::domain::event::MarketDataEvent;
use crate::ports::market_data_provider::MarketDataProvider;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::sync::mpsc::Sender;
use tokio::time::timeout;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct BinanceMarketProvider {
    cfg: BinanceConfig,
    runtime_cfg: BinanceRuntimeConfig,
}

impl BinanceMarketProvider {
    pub fn new(cfg: BinanceConfig, runtime_cfg: BinanceRuntimeConfig) -> Self {
        Self { cfg, runtime_cfg }
    }
}

#[async_trait]
impl MarketDataProvider for BinanceMarketProvider {
    async fn run(self: Box<Self>, market_tx: Sender<MarketDataEvent>, token: CancellationToken) {
        let (stream_tx, mut stream_rx) = mpsc::channel::<BinanceStreamData>(100);

        let public_url = self.cfg.public_ws_url();
        let public_runtime = self.runtime_cfg.clone();
        let public_tx = stream_tx.clone();
        let public_token = token.clone();
        let mut public_h = tokio::spawn(async move {
            subscribe_to_binance_futures_stream(
                public_url,
                "Public",
                public_runtime,
                public_tx,
                public_token,
            )
            .await
        });

        let market_url = self.cfg.market_ws_url();
        let market_runtime = self.runtime_cfg;
        let market_token = token.clone();
        let mut market_h = tokio::spawn(async move {
            subscribe_to_binance_futures_stream(
                market_url,
                "Market",
                market_runtime,
                stream_tx,
                market_token,
            )
            .await
        });

        let mut forward_h = tokio::spawn(async move {
            while let Some(stream) = stream_rx.recv().await {
                if let Err(e) = market_tx.send(stream.into()).await {
                    warn!(error = %e, "QuestDB Tx 채널 닫힘");
                    return;
                }
            }
        });

        let (task, result) = tokio::select! {
            r = &mut public_h => ("Public", r),
            r = &mut market_h => ("Market", r),
            r = &mut forward_h => ("Forward", r),
        };
        if let Err(e) = result {
            error!(task = task, error = ?e, "BinanceMarketProvider Task 비정상 종료");
        }

        token.cancel();
        public_h.abort();
        market_h.abort();
        forward_h.abort();
    }
}

pub async fn subscribe_to_binance_futures_stream(
    url: String,
    stream_type: &str,
    runtime_cfg: BinanceRuntimeConfig,
    tx: Sender<BinanceStreamData>,
    token: CancellationToken,
) {
    let label = format!("Binance {stream_type}");
    run_with_reconnect(
        BinanceMarketSession {
            url,
            tx,
            stream_type: stream_type.into(),
            read_timeout: Duration::from_secs(runtime_cfg.read_timeout_sec),
        },
        ReconnectConfig::defaults(label),
        token,
    )
    .await
}

struct BinanceMarketSession {
    url: String,
    stream_type: String,
    tx: Sender<BinanceStreamData>,
    read_timeout: Duration,
}

#[async_trait]
impl WsSession for BinanceMarketSession {
    async fn run_once(&mut self, token: CancellationToken) -> Result<(), WsError> {
        stream(
            &self.url,
            &self.stream_type,
            &self.tx,
            &token,
            self.read_timeout,
        )
        .await
        .map_err(WsError::retryable)
    }
}

async fn stream(
    url: &str,
    stream_type: &str,
    tx: &Sender<BinanceStreamData>,
    token: &CancellationToken,
    read_timeout: Duration,
) -> Result<()> {
    let request = url.into_client_request().context("잘못된 URL")?;
    let (ws_stream, _) = connect_async(request)
        .await
        .context(format!("Binance {stream_type} 연결 실패"))?;
    let (mut write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                let _ = write.send(Message::Close(None)).await;
                info!(stream_type = %stream_type, "Binance WS Close Frame 전송 후 종료");
                return Ok(());
            }
            msg = timeout(read_timeout, read.next()) => {
                let msg = match msg{
                    Ok(Some(m)) => m,
                    Ok(None) => return Err(anyhow!("Binance {stream_type} WS 스트림 종료")),
                    Err(_) => return Err(anyhow!("Binance {stream_type} WS 타임아웃 {}s", read_timeout.as_secs())),
                };
                match msg {
                    Ok(Message::Text(text)) => match serde_json::from_str::<BinanceStreamEnvelope>(&text) {
                        Ok(envelope) => {
                            if let Err(e) = tx.send(envelope.data).await {
                                warn!(stream_type = %stream_type, error = %e, "Binance Stream 채널 닫힘");
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            let preview: String = text.chars().take(200).collect();
                            warn!(stream_type = %stream_type, error = %e, raw_preview = %preview, "Binance Payload 파싱 실패");
                        }
                    },
                    Ok(Message::Ping(payload)) => {
                        write
                            .send(Message::Pong(payload))
                            .await
                            .context(format!("Binance {stream_type} Pong 전송 실패"))?;
                    }
                    Err(e) => return Err(e.into()),
                    _ => (),
                }
            }

        }
    }
}
