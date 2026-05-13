use std::time::Duration;

use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::{sync::mpsc::Sender, time::sleep};
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tracing::error;

use crate::dtos::{BookTickerData, DepthData, StreamData, StreamEnvelope, TradeData};

const WS_URL: &str = "wss://fstream.binance.com/stream?streams=btcusdt@trade/btcusdt@depth5@100ms/btcusdt@bookTicker";
const RECONNECT_DELAY_SECS: u64 = 5;

pub async fn subscribe_to_binance_futures_ws(
    trade_tx: Sender<TradeData>,
    depth_tx: Sender<DepthData>,
    book_tx: Sender<BookTickerData>,
) {
    loop {
        match stream(&trade_tx, &depth_tx, &book_tx).await {
            Ok(()) => break,
            Err(e) => {
                error!("WS 에러: {e}, {RECONNECT_DELAY_SECS}초 후 재시도");
                sleep(Duration::from_secs(RECONNECT_DELAY_SECS)).await;
            }
        }
    }
}

async fn stream(
    trade_tx: &Sender<TradeData>,
    depth_tx: &Sender<DepthData>,
    book_tx: &Sender<BookTickerData>,
) -> Result<()> {
    let request = WS_URL.into_client_request().context("잘못된 URL")?;
    let (ws_stream, _) = connect_async(request).await.context("연결 실패")?;

    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(envelope) = serde_json::from_str::<StreamEnvelope>(&text) {
                    let closed = match envelope.data {
                        StreamData::Trade(d) => trade_tx.send(d).await.is_err(),
                        StreamData::Depth(d) => depth_tx.send(d).await.is_err(),
                        StreamData::BookTicker(d) => book_tx.send(d).await.is_err(),
                    };

                    if closed {
                        return Ok(());
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

    Err(anyhow::anyhow!("WS 스트림 종료"))
}
