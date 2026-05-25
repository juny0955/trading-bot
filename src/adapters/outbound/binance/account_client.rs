use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::signing;
use crate::domain::account::{AccountError, AccountSnapshot, ExchangePositionSnapshot};
use crate::domain::order::OrderSide;
use crate::ports::account_source::AccountSource;
use async_trait::async_trait;
use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

/// Binance REST API에서 계정 정보 및 포지션을 조회하는 클라이언트.
pub struct BinanceAccountClient {
    api: Arc<BinanceOrderApi>,
}

impl BinanceAccountClient {
    /// 새 `BinanceAccountClient`를 생성한다.
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    async fn signed_post(&self, path: &str, params: &str) -> Result<Value, AccountError> {
        let ts = signing::timestamp_ms().map_err(|e| AccountError::Network(e.to_string()))?;
        let body = format!("{params}&timestamp={ts}&recvWindow=5000");
        let sig = signing::sign(&self.api.secret, &body)
            .map_err(|e| AccountError::Network(e.to_string()))?;
        let url = format!(
            "{}{}?{}&signature={}",
            self.api.rest_base, path, body, sig
        );
        let resp = self
            .api
            .client
            .post(&url)
            .header("X-MBX-APIKEY", &self.api.api_key)
            .send()
            .await
            .map_err(|e| AccountError::Network(e.to_string()))?;
        let status = resp.status();
        let val: Value = resp
            .json()
            .await
            .map_err(|e| AccountError::Parse(e.to_string()))?;
        if !status.is_success() {
            // -4046: No need to change margin type (already set) — treat as success
            if val["code"].as_i64() == Some(-4046) {
                return Ok(val);
            }
            return Err(AccountError::Network(format!("{status}: {val}")));
        }
        Ok(val)
    }

    async fn signed_get(&self, path: &str) -> Result<Value, AccountError> {
        let ts = signing::timestamp_ms().map_err(|e| AccountError::Network(e.to_string()))?;
        let query = format!("timestamp={ts}&recvWindow=5000");
        let sig = signing::sign(&self.api.secret, &query)
            .map_err(|e| AccountError::Network(e.to_string()))?;
        let url = format!("{}{}?{}&signature={}", self.api.rest_base, path, query, sig);
        let resp = self
            .api
            .client
            .get(&url)
            .header("X-MBX-APIKEY", &self.api.api_key)
            .send()
            .await
            .map_err(|e| AccountError::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| AccountError::Network(e.to_string()))?;
        resp.json::<Value>()
            .await
            .map_err(|e| AccountError::Parse(e.to_string()))
    }

    async fn signed_get_with_retry(&self, path: &str) -> Result<Value, AccountError> {
        let mut last: Option<AccountError> = None;
        for attempt in 0u32..3 {
            match self.signed_get(path).await {
                Ok(v) => return Ok(v),
                Err(e) => {
                    warn!(path, attempt, error = %e, "계정 API 호출 실패 — 재시도");
                    last = Some(e);
                    sleep(Duration::from_secs(1u64 << attempt)).await;
                }
            }
        }
        Err(AccountError::Unavailable(
            last.map(|e| e.to_string()).unwrap_or_default(),
        ))
    }
}

#[async_trait]
impl AccountSource for BinanceAccountClient {
    async fn fetch_account(&self) -> Result<AccountSnapshot, AccountError> {
        let v = self.signed_get_with_retry("/fapi/v2/account").await?;
        Ok(parse_account(&v))
    }

    async fn fetch_positions(&self) -> Result<Vec<ExchangePositionSnapshot>, AccountError> {
        let v = self.signed_get_with_retry("/fapi/v2/positionRisk").await?;
        Ok(parse_positions(&v))
    }

    async fn configure_symbol(
        &self,
        symbol: &str,
        leverage: i16,
        margin_type: &str,
    ) -> Result<(), AccountError> {
        self.signed_post(
            "/fapi/v1/marginType",
            &format!("symbol={symbol}&marginType={margin_type}"),
        )
        .await?;

        self.signed_post(
            "/fapi/v1/leverage",
            &format!("symbol={symbol}&leverage={leverage}"),
        )
        .await?;

        Ok(())
    }
}

fn parse_account(v: &Value) -> AccountSnapshot {
    AccountSnapshot {
        available_balance: decimal_str(&v["availableBalance"]).unwrap_or(Decimal::ZERO),
        total_wallet_balance: decimal_str(&v["totalWalletBalance"]).unwrap_or(Decimal::ZERO),
        total_unrealized_pnl: decimal_str(&v["totalUnrealizedProfit"]).unwrap_or(Decimal::ZERO),
        fetched_at: parse_update_time(&v["updateTime"]),
    }
}

fn parse_positions(v: &Value) -> Vec<ExchangePositionSnapshot> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter().filter_map(parse_one_position).collect()
}

fn parse_one_position(o: &Value) -> Option<ExchangePositionSnapshot> {
    let symbol = o["symbol"].as_str()?.to_string();
    let position_amt = decimal_str(&o["positionAmt"]).unwrap_or(Decimal::ZERO);
    if position_amt.is_zero() {
        return None;
    }
    let side = if position_amt > Decimal::ZERO {
        OrderSide::Buy
    } else {
        OrderSide::Sell
    };
    let qty = position_amt.abs();
    let entry_price = decimal_str(&o["entryPrice"]).unwrap_or(Decimal::ZERO);
    let leverage = o["leverage"]
        .as_str()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(0);
    Some(ExchangePositionSnapshot {
        symbol,
        side,
        qty,
        entry_price,
        leverage,
    })
}

fn decimal_str(v: &Value) -> Option<Decimal> {
    v.as_str().and_then(|s| s.parse::<Decimal>().ok())
}

fn parse_update_time(v: &Value) -> DateTime<Utc> {
    v.as_i64()
        .and_then(|ms| Utc.timestamp_millis_opt(ms).single())
        .unwrap_or_else(Utc::now)
}
