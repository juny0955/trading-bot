use crate::adapters::binance::api::BinanceOrderApi;
use crate::domain::exchange_info::SymbolFilters;
use crate::ports::exchange_info_source::{ExchangeInfoError, ExchangeInfoSource};
use async_trait::async_trait;
use rust_decimal::Decimal;
use serde_json::Value;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::sleep;
use tracing::warn;

/// Binance REST API에서 거래소 심볼 정보를 조회하는 클라이언트.
pub struct BinanceExchangeInfoClient {
    api: Arc<BinanceOrderApi>,
}

impl BinanceExchangeInfoClient {
    /// 새 `BinanceExchangeInfoClient`를 생성한다.
    pub fn new(api: Arc<BinanceOrderApi>) -> Self {
        Self { api }
    }

    async fn fetch_once(&self) -> Result<Value, ExchangeInfoError> {
        let url = format!("{}/fapi/v1/exchangeInfo", self.api.rest_base);
        let resp = self
            .api
            .client
            .get(&url)
            .send()
            .await
            .map_err(|e| ExchangeInfoError::Network(e.to_string()))?
            .error_for_status()
            .map_err(|e| ExchangeInfoError::Network(e.to_string()))?;
        resp.json::<Value>()
            .await
            .map_err(|e| ExchangeInfoError::Parse(e.to_string()))
    }
}

#[async_trait]
impl ExchangeInfoSource for BinanceExchangeInfoClient {
    async fn fetch_all(&self) -> Result<Vec<SymbolFilters>, ExchangeInfoError> {
        let mut last_err: Option<ExchangeInfoError> = None;
        for attempt in 0u32..3 {
            match self.fetch_once().await {
                Ok(v) => return Ok(parse_symbols(&v)),
                Err(e) => {
                    warn!(attempt, error = %e, "exchangeInfo 호출 실패 — 재시도");
                    last_err = Some(e);
                    sleep(Duration::from_secs(1u64 << attempt)).await;
                }
            }
        }
        Err(ExchangeInfoError::Unavailable(
            last_err.map(|e| e.to_string()).unwrap_or_default(),
        ))
    }
}

fn parse_symbols(v: &Value) -> Vec<SymbolFilters> {
    let Some(symbols) = v["symbols"].as_array() else {
        return Vec::new();
    };
    symbols.iter().filter_map(parse_symbol).collect()
}

fn parse_symbol(s: &Value) -> Option<SymbolFilters> {
    if s["status"].as_str() != Some("TRADING") {
        return None;
    }
    let symbol = s["symbol"].as_str()?.to_string();
    let filters = s["filters"].as_array()?;

    let mut min_qty = Decimal::ZERO;
    let mut step_size = Decimal::ZERO;
    let mut tick_size = Decimal::ZERO;
    let mut min_notional = Decimal::ZERO;
    for f in filters {
        let kind = f["filterType"].as_str().unwrap_or("");
        match kind {
            "LOT_SIZE" | "MARKET_LOT_SIZE" => {
                if let Some(v) = decimal_str(&f["stepSize"]) {
                    step_size = v;
                }
                if let Some(v) = decimal_str(&f["minQty"])
                    && (min_qty.is_zero() || v > min_qty)
                {
                    min_qty = v;
                }
            }
            "PRICE_FILTER" => {
                if let Some(v) = decimal_str(&f["tickSize"]) {
                    tick_size = v;
                }
            }
            "MIN_NOTIONAL" => {
                if let Some(v) = decimal_str(&f["notional"]) {
                    min_notional = v;
                }
            }
            _ => {}
        }
    }

    Some(SymbolFilters {
        symbol,
        min_notional,
        min_qty,
        step_size,
        tick_size,
        max_leverage: 0,
    })
}

fn decimal_str(v: &Value) -> Option<Decimal> {
    v.as_str().and_then(|s| s.parse::<Decimal>().ok())
}
