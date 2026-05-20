use crate::config::binance_config::BinanceConfig;
use crate::domain::order::OrderError;
use anyhow::{Result, anyhow};
use reqwest::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time;
use tracing::warn;

#[derive(Debug, thiserror::Error)]
pub enum BinanceApiError {
    #[error("주문 거부: code={code} msg={msg}")]
    Rejected { code: i32, msg: String },
    #[error("timeout")]
    Timeout,
    #[error("connection: {0}")]
    Connection(String),
}

impl From<BinanceApiError> for OrderError {
    fn from(value: BinanceApiError) -> Self {
        match value {
            // -2010, -2018 같은 증거금 코드만 InsufficientMargin으로
            BinanceApiError::Rejected { code: -2010 | -2018 | -2019, .. } => Self::InsufficientMargin,
            BinanceApiError::Rejected { code, msg } => Self::ExchangeRejected { code, msg },
            BinanceApiError::Timeout => Self::Timeout,
            BinanceApiError::Connection(s) => Self::ConnectionLost(s),
        }
    }
}

pub struct BinanceOrderApi {
    pub(crate) api_key: String,
    pub(crate) secret: String,
    pub(crate) order_ws_url: String,
    pub(crate) user_stream_base: String,
    pub(crate) rest_base: String,
    pub(crate) ws_tx: Sender<String>,
    pub(crate) pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
    client: Client,
}

impl BinanceOrderApi {
    pub fn new(
        api_key: String,
        secret: String,
        cfg: &BinanceConfig,
    ) -> Result<(Arc<Self>, Receiver<String>)> {
        anyhow::ensure!(!api_key.trim().is_empty(), "BINANCE_API_KEY 비어있음");
        anyhow::ensure!(!secret.trim().is_empty(), "BINANCE_SECRET 비어있음");
        let (ws_tx, ws_rx) = mpsc::channel(100);

        let client = Client::builder()
            .timeout(Duration::from_secs(10))
            .build()?;

        let executor = Arc::new(Self {
            api_key,
            secret,
            order_ws_url: cfg.order_ws_url().to_string(),
            user_stream_base: cfg.user_stream_base_url().to_string(),
            rest_base: cfg.rest_base_url().to_string(),
            ws_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
            client,
        });
        Ok((executor, ws_rx))
    }

    pub(crate) async fn send_and_wait(
        &self,
        id: String,
        request: Value,
    ) -> Result<Value, BinanceApiError> {
        let (resp_tx, resp_rx) = oneshot::channel();

        let payload = serde_json::to_string(&request)
            .map_err(|e| BinanceApiError::Connection(e.to_string()))?;

        {
            let mut pending = self.pending.lock().await;
            pending.insert(id.clone(), resp_tx);
        }

        if let Err(e) = self.ws_tx.send(payload).await {
            self.pending.lock().await.remove(&id);
            warn!("Order WS 송신 실패 (id={id}): {e}");
            return Err(BinanceApiError::Connection(e.to_string()));
        }

        match time::timeout(Duration::from_secs(5), resp_rx).await {
            Ok(Ok(mut val)) => {
                if val["status"].as_u64() != Some(200) {
                    let code = val["error"]["code"].as_i64().unwrap_or(-1) as i32;
                    let msg = val["error"]["msg"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();

                    warn!("Order WS 거래소 거부 (id={id}, code={code}): {msg}");
                    return Err(BinanceApiError::Rejected { code, msg });
                }
                Ok(val["result"].take())
            }
            Ok(Err(_)) => {
                warn!("Order WS 채널 닫힘 (id={id})");
                Err(BinanceApiError::Connection("channel closed".into()))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                warn!("Order WS 응답 timeout (id={id})");
                Err(BinanceApiError::Connection("timeout".into()))
            }
        }
    }

    pub(crate) async fn get_listen_key(&self) -> Result<String> {
        let url = format!("{}/fapi/v1/listenKey", self.rest_base);
        let val = self
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .send()
            .await?
            .json::<Value>()
            .await?;

        val["listenKey"]
            .as_str()
            .map(|s| s.to_string())
            .ok_or_else(|| anyhow!("listenKey 없음"))
    }

    pub(crate) async fn renew_listen_key(&self, listen_key: &str) -> Result<()> {
        let url = format!("{}/fapi/v1/listenKey", self.rest_base);
        self.client
            .put(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key)])
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
