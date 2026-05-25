use crate::adapters::binance::api::BinanceOrderApi;
use crate::adapters::binance::signing;
use crate::domain::account::{AccountError, AccountSnapshot, ExchangePositionSnapshot};
use crate::domain::order::OrderSide;
use crate::ports::account_source::AccountSource;
use async_trait::async_trait;
use chrono::Utc;
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
        let (_body, url) =
            build_post_body_and_url(&self.api.secret, &self.api.rest_base, path, params)?;
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
            // -4046: 이미 설정된 마진 타입 변경 시도 → 거래소가 에러로 응답하지만 실제로는 정상
            if val["code"].as_i64() == Some(-4046) {
                return Ok(val);
            }
            return Err(AccountError::Network(format!("{status}: {val}")));
        }
        Ok(val)
    }

    async fn signed_get(&self, path: &str) -> Result<Value, AccountError> {
        let url = build_get_url(&self.api.secret, &self.api.rest_base, path)?;
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
        // 재시도 지연: 1s → 2s → 4s (지수 백오프)
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
    /// Binance `/fapi/v2/account`에서 계정 잔액 스냅샷을 조회한다.
    async fn fetch_account(&self) -> Result<AccountSnapshot, AccountError> {
        let v = self.signed_get_with_retry("/fapi/v2/account").await?;
        Ok(parse_account(&v))
    }

    /// Binance `/fapi/v2/positionRisk`에서 비-제로 포지션 목록을 조회한다.
    async fn fetch_positions(&self) -> Result<Vec<ExchangePositionSnapshot>, AccountError> {
        let v = self.signed_get_with_retry("/fapi/v2/positionRisk").await?;
        Ok(parse_positions(&v))
    }

    /// 심볼의 마진 타입과 레버리지를 거래소에 설정한다.
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

/// POST 본문과 서명이 포함된 전체 URL을 구성한다.
///
/// 반환값은 `(body, url)` 쌍이며, body는 HTTP 바디 문자열,
/// url은 타임스탬프·서명이 쿼리 파라미터로 포함된 전체 URL이다.
fn build_post_body_and_url(
    secret: &str,
    rest_base: &str,
    path: &str,
    params: &str,
) -> Result<(String, String), AccountError> {
    let ts = signing::timestamp_ms().map_err(|e| AccountError::Network(e.to_string()))?;
    let body = format!("{params}&timestamp={ts}&recvWindow=5000");
    let sig = signing::sign(secret, &body).map_err(|e| AccountError::Network(e.to_string()))?;
    let url = format!("{rest_base}{path}?{body}&signature={sig}");
    Ok((body, url))
}

/// 타임스탬프·서명이 포함된 GET 요청 URL을 구성한다.
fn build_get_url(
    secret: &str,
    rest_base: &str,
    path: &str,
) -> Result<String, AccountError> {
    let ts = signing::timestamp_ms().map_err(|e| AccountError::Network(e.to_string()))?;
    let query = format!("timestamp={ts}&recvWindow=5000");
    let sig = signing::sign(secret, &query).map_err(|e| AccountError::Network(e.to_string()))?;
    Ok(format!("{rest_base}{path}?{query}&signature={sig}"))
}

/// Binance `/fapi/v2/account` 응답을 `AccountSnapshot`으로 변환한다.
///
/// `availableBalance`, `totalWalletBalance`, `totalUnrealizedProfit` 필드를 읽으며,
/// 누락되거나 파싱에 실패하는 필드는 `Decimal::ZERO`로 대체된다.
fn parse_account(v: &Value) -> AccountSnapshot {
    AccountSnapshot {
        available_balance: decimal_str(&v["availableBalance"]).unwrap_or(Decimal::ZERO),
        total_wallet_balance: decimal_str(&v["totalWalletBalance"]).unwrap_or(Decimal::ZERO),
        total_unrealized_pnl: decimal_str(&v["totalUnrealizedProfit"]).unwrap_or(Decimal::ZERO),
        fetched_at: Utc::now(),
    }
}

/// Binance `/fapi/v2/positionRisk` 응답 배열을 포지션 스냅샷 목록으로 변환한다.
///
/// 입력이 배열이 아니거나 파싱에 실패하면 빈 `Vec`을 반환한다.
/// `positionAmt`가 0인 항목은 건너뛴다.
fn parse_positions(v: &Value) -> Vec<ExchangePositionSnapshot> {
    let Some(arr) = v.as_array() else {
        return Vec::new();
    };
    arr.iter().filter_map(parse_one_position).collect()
}

/// 포지션 배열의 단일 항목을 `ExchangePositionSnapshot`으로 변환한다.
///
/// `positionAmt`가 0이면 `None`을 반환한다.
/// 양수이면 `Buy`, 음수이면 `Sell` 방향으로 해석하고 수량은 절댓값을 사용한다.
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

/// JSON 문자열 값을 `Decimal`로 파싱한다. 파싱 실패 시 `None`을 반환한다.
fn decimal_str(v: &Value) -> Option<Decimal> {
    v.as_str().and_then(|s| s.parse::<Decimal>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    #[test]
    fn parse_account_extracts_balances() -> anyhow::Result<()> {
        let v = json!({
            "availableBalance": "100.5",
            "totalWalletBalance": "200.0",
            "totalUnrealizedProfit": "10.0"
        });
        let snap = parse_account(&v);
        assert_eq!(snap.available_balance, dec!(100.5));
        assert_eq!(snap.total_wallet_balance, dec!(200.0));
        assert_eq!(snap.total_unrealized_pnl, dec!(10.0));
        Ok(())
    }

    #[test]
    fn parse_account_missing_fields_default_to_zero() -> anyhow::Result<()> {
        // 누락된 필드는 Decimal::ZERO로 대체된다
        let snap = parse_account(&json!({}));
        assert_eq!(snap.available_balance, Decimal::ZERO);
        assert_eq!(snap.total_wallet_balance, Decimal::ZERO);
        assert_eq!(snap.total_unrealized_pnl, Decimal::ZERO);
        Ok(())
    }

    #[test]
    fn parse_positions_filters_zero_amount() -> anyhow::Result<()> {
        let v = json!([
            {"symbol": "BTCUSDT", "positionAmt": "0.0", "entryPrice": "100", "leverage": "10"},
            {"symbol": "ETHUSDT", "positionAmt": "1.5", "entryPrice": "200", "leverage": "20"},
        ]);
        let snaps = parse_positions(&v);
        // positionAmt == 0인 BTCUSDT는 제외되어야 한다
        assert_eq!(snaps.len(), 1);
        assert_eq!(snaps[0].symbol, "ETHUSDT");
        Ok(())
    }

    #[test]
    fn parse_one_position_negative_amt_is_sell() -> anyhow::Result<()> {
        let o = json!({
            "symbol": "BTCUSDT",
            "positionAmt": "-2.0",
            "entryPrice": "50000",
            "leverage": "5"
        });
        let snap = parse_one_position(&o).ok_or_else(|| anyhow::anyhow!("Some 예상"))?;
        assert_eq!(snap.side, OrderSide::Sell);
        // 수량은 절댓값
        assert_eq!(snap.qty, dec!(2.0));
        Ok(())
    }
}
