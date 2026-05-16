use std::time::Duration;

use crate::config::BinanceRuntimeConfig;
use crate::market_data::binance::dto::{StreamData, StreamEnvelope};
use anyhow::{Context, Result, anyhow};
use futures_util::{SinkExt, StreamExt};
use tokio::time::timeout;
use tokio::{sync::mpsc::Sender, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const READ_TIMEOUT: Duration = Duration::from_secs(60);

pub async fn subscribe_to_binance_futures_ws(
    url: String,
    runtime_cfg: BinanceRuntimeConfig,
    tx: Sender<StreamData>,
    token: CancellationToken,
) {
    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!("Binance WS 스트림 종료");
                return;
            },
            res = stream(&url, &tx, &token) => match res {
                Ok(()) => break,
                Err(e) => {
                    error!(
                        "Binance WS 에러: {e}, {}초 후 재시도",
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

async fn stream(url: &str, tx: &Sender<StreamData>, token: &CancellationToken) -> Result<()> {
    let request = url.into_client_request().context("잘못된 URL")?;
    let (ws_stream, _) = connect_async(request).await.context("연결 실패")?;
    let (mut write, mut read) = ws_stream.split();

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                let _ = write.send(Message::Close(None)).await;
                info!("Binance Public WS Close Frame 전송 후 종료");
                return Ok(());
            }
            msg = timeout(READ_TIMEOUT, read.next()) => {
                let msg = match msg{
                    Ok(Some(m)) => m,
                    Ok(None) => return Err(anyhow!("Binance Public WS 스트림 종료")),
                    Err(_) => return Err(anyhow!("Binance Public WS 타임아웃 {}s", READ_TIMEOUT.as_secs())),
                };
                match msg {
                    Ok(Message::Text(text)) => match serde_json::from_str::<StreamEnvelope>(&text) {
                        Ok(envelope) => {
                            if let Err(e) = tx.send(envelope.data).await {
                                warn!("Binance Public Stream 채널 닫힘: {e}");
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            let preview: String = text.chars().take(200).collect();
                            warn!("Binance Public Payload 파싱 실패: {e}, raw={preview}");
                        }
                    },
                    Ok(Message::Ping(payload)) => {
                        write
                            .send(Message::Pong(payload))
                            .await
                            .context("Binance Public Pong 전송 실패")?;
                    }
                    Err(e) => return Err(e.into()),
                    _ => (),
                }
            }

        }
    }
}
