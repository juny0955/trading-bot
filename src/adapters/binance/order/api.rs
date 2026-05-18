use crate::domain::order::OrderError;
use anyhow::anyhow;
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc::{Receiver, Sender};
use tokio::sync::{Mutex, mpsc, oneshot};
use tokio::time;
use tracing::warn;

pub struct BinanceOrderApi {
    pub(crate) api_key: String,
    pub(crate) secret: String,
    pub(crate) order_ws_url: String,
    pub(crate) user_stream_base: String,
    pub(crate) rest_base: String,
    pub(crate) ws_tx: Sender<String>,
    pub(crate) pending: Arc<Mutex<HashMap<String, oneshot::Sender<Value>>>>,
}

impl BinanceOrderApi {
    pub fn new(api_key: String, secret: String, testnet: bool) -> (Arc<Self>, Receiver<String>) {
        let (ws_tx, ws_rx) = mpsc::channel(100);
        let order_ws_url = if testnet {
            "wss://testnet.binancefuture.com/ws-fapi/v1".to_string()
        } else {
            "wss://ws-fapi.binance.com/ws-fapi/v1".to_string()
        };

        let user_stream_base = if testnet {
            "wss://stream.binancefuture.com/ws".to_string()
        } else {
            "wss://fstream.binance.com/ws".to_string()
        };

        let rest_base = if testnet {
            "https://testnet.binancefuture.com".to_string()
        } else {
            "https://fapi.binance.com".to_string()
        };

        let executor = Arc::new(Self {
            api_key,
            secret,
            order_ws_url,
            user_stream_base,
            rest_base,
            ws_tx,
            pending: Arc::new(Mutex::new(HashMap::new())),
        });
        (executor, ws_rx)
    }

    pub(crate) async fn send_and_wait(
        &self,
        id: String,
        request: Value,
    ) -> Result<Value, OrderError> {
        let (resp_tx, resp_rx) = oneshot::channel();
        self.pending.lock().await.insert(id.clone(), resp_tx);

        if let Err(e) = self.ws_tx.send(request.to_string()).await {
            self.pending.lock().await.remove(&id);
            warn!("Order WS 송신 실패 (id={id}): {e}");
            return Err(OrderError::Connection(e.to_string()));
        }

        match time::timeout(Duration::from_secs(5), resp_rx).await {
            Ok(Ok(val)) => {
                if val["status"].as_u64() != Some(200) {
                    let code = val["error"]["code"].as_i64().unwrap_or(-1) as i32;
                    let msg = val["error"]["msg"]
                        .as_str()
                        .unwrap_or("unknown")
                        .to_string();
                    warn!("Order WS 거래소 거부 (id={id}, code={code}): {msg}");
                    return Err(OrderError::ExchangeRejected { code, msg });
                }
                Ok(val["result"].clone())
            }
            Ok(Err(_)) => {
                warn!("Order WS 채널 닫힘 (id={id})");
                Err(OrderError::Connection("channel closed".into()))
            }
            Err(_) => {
                self.pending.lock().await.remove(&id);
                warn!("Order WS 응답 timeout (id={id})");
                Err(OrderError::Connection("timeout".into()))
            }
        }
    }

    pub(crate) async fn get_listen_key(&self) -> anyhow::Result<String> {
        let url = format!("{}/fapi/v1/listenKey", self.rest_base);
        let val = reqwest::Client::new()
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

    pub(crate) async fn renew_listen_key(&self, listen_key: &str) -> anyhow::Result<()> {
        let url = format!("{}/fapi/v1/listenKey", self.rest_base);
        reqwest::Client::new()
            .put(&url)
            .header("X-MBX-APIKEY", &self.api_key)
            .query(&[("listenKey", listen_key)])
            .send()
            .await?
            .error_for_status()?;

        Ok(())
    }
}
