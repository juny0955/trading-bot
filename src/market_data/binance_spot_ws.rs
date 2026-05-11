use anyhow::{Context, Result};
use futures_util::StreamExt;
use rust_decimal::Decimal;
use serde::Deserialize;
use tokio::sync::mpsc::Sender;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message, client::IntoClientRequest},
};
use tracing::{debug, error};

#[derive(Debug, Deserialize)]
pub struct Ticker {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "c")]
    pub price: Decimal,
    #[serde(rename = "E")]
    pub event_time: u64,
}

pub async fn subscribe_to_binance_spot_ws(tx: Sender<Ticker>) -> Result<()> {
    let request = "wss://stream.binance.com:9443/ws/btcusdt@ticker"
        .into_client_request()
        .context("잘못된 URL")?;
    let (ws_stream, _) = connect_async(request).await.context("연결 실패")?;

    let (_, mut read) = ws_stream.split();

    while let Some(msg) = read.next().await {
        match msg {
            Ok(Message::Text(text)) => {
                if let Ok(ticker) = serde_json::from_str::<Ticker>(&text)
                    && tx.send(ticker).await.is_err()
                {
                    break;
                }
            }
            Ok(Message::Ping(_payload)) => {
                debug!("Ping 수신");
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
