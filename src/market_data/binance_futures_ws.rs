use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::{sync::mpsc::Sender, time::sleep};
use tokio::time::error::Elapsed;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_tungstenite::tungstenite::Error;
use tracing::{error, warn};

use crate::{
    BinanceConfig, BinanceRuntimeConfig,
    dtos::{StreamData, StreamEnvelope},
};

const READ_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn subscribe_to_binance_futures_ws(
    cfg: BinanceConfig,
    runtime_cfg: BinanceRuntimeConfig,
    tx: Sender<StreamData>,
) {
    loop {
        match stream(&cfg.ws_url(), &tx).await {
            Ok(()) => break,
            Err(e) => {
                error!(
                    "WS 에러: {e}, {}초 후 재시도",
                    runtime_cfg.reconnect_delay_sec
                );
                sleep(Duration::from_secs(runtime_cfg.reconnect_delay_sec)).await;
            }
        }
    }
}

async fn stream(url: &str, tx: &Sender<StreamData>) -> Result<()> {
    let request = url.into_client_request().context("잘못된 URL")?;
    let (ws_stream, _) = connect_async(request).await.context("연결 실패")?;
    let (mut write, mut read) = ws_stream.split();

    loop {
        let msg = match tokio::time::timeout(READ_TIMEOUT, read.next()).await {
            Ok(Some(m)) => m,
            Ok(None) => return Err(anyhow!("WS 스트림 종료")),
            Err(_) => return Err(anyhow!("WS 타임아웃 {}s", READ_TIMEOUT.as_secs())),
        };

        match msg {
            Ok(Message::Text(text)) => {
                match serde_json::from_str::<StreamEnvelope>(&text) {
                    Ok(envelope) => {
                        if let Err(e) = tx.send(envelope.data).await {
                            warn!("바이낸스 Stream 채널 닫힘: {e}");
                            return Ok(())
                        }
                    }
                    Err(e) => {
                        let preview: String = text.chars().take(200).collect();
                        warn!("payload 파싱 실패: {e}, raw={preview}");
                    }
                }
            }
            Ok(Message::Ping(payload)) => {
                write
                    .send(Message::Pong(payload))
                    .await
                    .context("Pong 전송 실패")?;
            }
            Err(e) => return Err(e.into()),
            _ => (),
        }
    }
}
