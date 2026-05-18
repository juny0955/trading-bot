use anyhow::anyhow;
use serde_json::Value;

use crate::binance::LiveOrderExecutor;

impl LiveOrderExecutor {
    pub(super) async fn get_listen_key(&self) -> anyhow::Result<String> {
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

    pub(super) async fn renew_listen_key(&self, listen_key: &str) -> anyhow::Result<()> {
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
