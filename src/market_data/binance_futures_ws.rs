use anyhow::{Context, Result};
use futures_util::{SinkExt, StreamExt};
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tracing::error;

use crate::dtos::{BookTickerData, DepthData, StreamData, StreamEnvelope, TradeData};

pub async fn subscribe_to_binance_futures_ws(
    trade_tx: Sender<TradeData>,
    depth_tx: Sender<DepthData>,
    book_tx: Sender<BookTickerData>,
) -> Result<()> {
    let request = "wss://fstream.binance.com/stream?streams=btcusdt@trade/btcusdt@depth5@100ms/btcusdt@bookTicker"
        .into_client_request()
        .context("잘못된 URL")?;
    let (ws_stream, _) = connect_async(request).await.context("연결 실패")?;

    let (mut write, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(envelope) = serde_json::from_str::<StreamEnvelope>(&text) {
                    match envelope.data {
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
            }
            Ok(Message::Ping(payload)) => {
                if let Err(e) = write.send(Message::Pong(payload)).await {
                    error!("Pong 전송 실패: {e}");
                    break;
                }
            }
            Err(e) => {
                error!("에러 발생: {e}");
                break;
            }
            _ => (),
        }
    }

    Ok(())
}
