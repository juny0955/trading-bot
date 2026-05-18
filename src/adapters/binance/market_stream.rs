use std::time::Duration;

use crate::adapters::binance::dto::{StreamData, StreamEnvelope};
use crate::config::BinanceRuntimeConfig;
use crate::config::binance_config::BinanceConfig;
use crate::domain::event::MarketDataEvent;
use crate::ports::live_market_feed::LiveMarketFeed;
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use futures_util::future::join_all;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc;
use tokio::time::timeout;
use tokio::{sync::mpsc::Sender, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub struct BinanceLiveFeed {
    cfg: BinanceConfig,
    runtime_cfg: BinanceRuntimeConfig,
}

impl BinanceLiveFeed {
    pub fn new(cfg: BinanceConfig, runtime_cfg: BinanceRuntimeConfig) -> Self {
        Self { cfg, runtime_cfg }
    }
}

#[async_trait]
impl LiveMarketFeed for BinanceLiveFeed {
    async fn run(self: Box<Self>, market_tx: Sender<MarketDataEvent>, token: CancellationToken) {
        let (stream_tx, mut stream_rx) = mpsc::channel::<StreamData>(100);
        let mut handles = Vec::new();

        let public_url = self.cfg.public_ws_url();
        let public_runtime = self.runtime_cfg.clone();
        let public_tx = stream_tx.clone();
        let public_token = token.clone();
        handles.push(tokio::spawn(async move {
            subscribe_to_binance_futures_stream(
                public_url,
                "Public",
                public_runtime,
                public_tx,
                public_token,
            )
            .await
        }));

        let market_url = self.cfg.market_ws_url();
        let market_runtime = self.runtime_cfg;
        let market_token = token.clone();
        handles.push(tokio::spawn(async move {
            subscribe_to_binance_futures_stream(
                market_url,
                "Market",
                market_runtime,
                stream_tx,
                market_token,
            )
            .await
        }));

        handles.push(tokio::spawn(async move {
            while let Some(stream) = stream_rx.recv().await {
                if let Err(e) = market_tx.send(stream.into()).await {
                    warn!("QuestDB Tx 채널 닫힘: {e}");
                    return;
                }
            }
        }));

        let _ = join_all(handles).await;
    }
}

pub async fn subscribe_to_binance_futures_stream(
    url: String,
    stream_type: &str,
    runtime_cfg: BinanceRuntimeConfig,
    tx: Sender<StreamData>,
    token: CancellationToken,
) {
    let read_timeout = Duration::from_secs(runtime_cfg.read_timeout_sec);

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!("Binance {stream_type} WS 스트림 종료");
                return;
            },
            res = stream(&url, stream_type, &tx, &token, read_timeout) => match res {
                Ok(()) => break,
                Err(e) => {
                    error!(
                        "Binance {stream_type} WS 에러: {e}, {}초 후 재시도",
                        runtime_cfg.reconnect_delay_sec
                    );
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = sleep(Duration::from_secs(runtime_cfg.reconnect_delay_sec)) => {}
                    }
                }
            }
        }
    }
}

async fn stream(
    url: &str,
    stream_type: &str,
    tx: &Sender<StreamData>,
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
                info!("Binance {stream_type} WS Close Frame 전송 후 종료");
                return Ok(());
            }
            msg = timeout(read_timeout, read.next()) => {
                let msg = match msg{
                    Ok(Some(m)) => m,
                    Ok(None) => return Err(anyhow!("Binance {stream_type} WS 스트림 종료")),
                    Err(_) => return Err(anyhow!("Binance {stream_type} WS 타임아웃 {}s", read_timeout.as_secs())),
                };
                match msg {
                    Ok(Message::Text(text)) => match serde_json::from_str::<StreamEnvelope>(&text) {
                        Ok(envelope) => {
                            if let Err(e) = tx.send(envelope.data).await {
                                warn!("Binance {stream_type} Stream 채널 닫힘: {e}");
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            let preview: String = text.chars().take(200).collect();
                            warn!("Binance {stream_type} Payload 파싱 실패: {e}, raw={preview}");
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
