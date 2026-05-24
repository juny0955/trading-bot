use crate::application::order_execution_event::{FillData, OrderExecutionEvent};
use crate::domain::order::{OrderSide, OrderStatus};
use chrono::{DateTime, TimeZone, Utc};
use rust_decimal::Decimal;
use serde_json::Value;
use thiserror::Error;

/// Binance 계정 이벤트 DTO 변환 오류입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::adapters::binance::dto_account::DtoMappingError;
///
/// let err = DtoMappingError::MissingField("o.i");
/// assert!(err.to_string().contains("missing field"));
/// ```
#[derive(Debug, Error)]
pub enum DtoMappingError {
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("parse error: {0}")]
    Parse(String),
}

/// Binance `ORDER_TRADE_UPDATE` payload의 주문 갱신 데이터입니다.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use trading_bot::adapters::binance::dto_account::BinanceOrderTradeUpdate;
///
/// let dto = BinanceOrderTradeUpdate::from_value(&json!({
///     "o": { "i": 1, "x": "NEW", "X": "NEW", "S": "BUY" }
/// })).unwrap();
/// assert_eq!(dto.exchange_order_id, 1);
/// ```
pub struct BinanceOrderTradeUpdate {
    pub exchange_order_id: i64,
    pub execution_type: String, // "TRADE", "NEW", etc.
    pub status: String,         // "FILLED", "PARTIALLY_FILLED", etc.
    pub side: String,
    pub last_filled_qty: Decimal,
    pub last_fill_price: Decimal,
    pub fee: Decimal,
    pub fee_asset: String,
    pub trade_time_ms: i64,
    pub cumulative_filled_qty: Decimal,
    pub avg_fill_price: Decimal,
}

impl BinanceOrderTradeUpdate {
    /// JSON payload에서 주문 갱신 DTO를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use trading_bot::adapters::binance::dto_account::BinanceOrderTradeUpdate;
    ///
    /// let dto = BinanceOrderTradeUpdate::from_value(&json!({
    ///     "o": { "i": 1, "x": "TRADE", "X": "FILLED", "S": "SELL", "l": "1" }
    /// })).unwrap();
    /// assert_eq!(dto.side, "SELL");
    /// ```
    pub fn from_value(val: &Value) -> Result<Self, DtoMappingError> {
        let o = &val["o"];
        Ok(Self {
            exchange_order_id: required_i64(o, "i", "o.i")?,
            execution_type: required_string(o, "x", "o.x")?,
            status: required_string(o, "X", "o.X")?,
            side: required_string(o, "S", "o.S")?,
            last_filled_qty: decimal_or_zero(o, "l", "o.l")?,
            last_fill_price: decimal_or_zero(o, "L", "o.L")?,
            fee: decimal_or_zero(o, "n", "o.n")?,
            fee_asset: string_or(o, "N", "USDT"),
            trade_time_ms: o["T"].as_i64().unwrap_or_default(),
            cumulative_filled_qty: decimal_or_zero(o, "z", "o.z")?,
            avg_fill_price: decimal_or_zero(o, "ap", "o.ap")?,
        })
    }

    /// 내부 주문 실행 이벤트로 변환합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use serde_json::json;
    /// use trading_bot::adapters::binance::dto_account::BinanceOrderTradeUpdate;
    /// use trading_bot::domain::order::OrderStatus;
    ///
    /// let event = BinanceOrderTradeUpdate::from_value(&json!({
    ///     "o": { "i": 1, "x": "NEW", "X": "FILLED", "S": "BUY", "z": "1", "ap": "100" }
    /// })).unwrap().into_execution_event();
    /// assert_eq!(event.new_status, Some(OrderStatus::Filled));
    /// ```
    pub fn into_execution_event(self) -> OrderExecutionEvent {
        OrderExecutionEvent {
            exchange_order_id: self.exchange_order_id,
            fill: fill_data(&self),
            new_status: order_status(&self.status),
            cumulative_filled_qty: self.cumulative_filled_qty,
            avg_fill_price: non_zero_decimal(self.avg_fill_price),
        }
    }
}

fn required_i64(
    value: &Value,
    key: &'static str,
    label: &'static str,
) -> Result<i64, DtoMappingError> {
    value[key]
        .as_i64()
        .ok_or(DtoMappingError::MissingField(label))
}

fn required_string(
    value: &Value,
    key: &'static str,
    label: &'static str,
) -> Result<String, DtoMappingError> {
    value[key]
        .as_str()
        .map(str::to_string)
        .ok_or(DtoMappingError::MissingField(label))
}

fn string_or(value: &Value, key: &str, default: &str) -> String {
    value[key].as_str().unwrap_or(default).to_string()
}

fn decimal_or_zero(value: &Value, key: &str, label: &str) -> Result<Decimal, DtoMappingError> {
    value[key]
        .as_str()
        .unwrap_or("0")
        .parse()
        .map_err(|e| DtoMappingError::Parse(format!("{label}: {e}")))
}

fn fill_data(update: &BinanceOrderTradeUpdate) -> Option<FillData> {
    if update.execution_type != "TRADE" {
        return None;
    }

    Some(FillData {
        side: order_side(&update.side),
        qty: update.last_filled_qty,
        price: update.last_fill_price,
        fee: update.fee,
        fee_asset: update.fee_asset.clone(),
        filled_at: trade_time(update.trade_time_ms),
    })
}

fn order_side(side: &str) -> OrderSide {
    match side {
        "SELL" => OrderSide::Sell,
        _ => OrderSide::Buy,
    }
}

fn order_status(status: &str) -> Option<OrderStatus> {
    match status {
        "FILLED" => Some(OrderStatus::Filled),
        "PARTIALLY_FILLED" => Some(OrderStatus::PartiallyFilled),
        "CANCELED" => Some(OrderStatus::Cancelled),
        "EXPIRED" => Some(OrderStatus::Expired),
        "REJECTED" => Some(OrderStatus::Rejected),
        _ => None,
    }
}

fn non_zero_decimal(value: Decimal) -> Option<Decimal> {
    if value.is_zero() { None } else { Some(value) }
}

fn trade_time(trade_time_ms: i64) -> DateTime<Utc> {
    Utc.timestamp_millis_opt(trade_time_ms)
        .single()
        .unwrap_or_else(Utc::now)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use serde_json::json;

    #[test]
    fn trade_update_maps_fill_and_status() -> anyhow::Result<()> {
        let update = BinanceOrderTradeUpdate::from_value(&json!({
            "o": {
                "i": 42, "x": "TRADE", "X": "FILLED", "S": "SELL",
                "l": "1.2", "L": "100.5", "n": "0.01", "N": "USDT",
                "T": 1700000000000i64, "z": "1.2", "ap": "100.5"
            }
        }))?;

        let event = update.into_execution_event();
        assert_eq!(event.new_status, Some(OrderStatus::Filled));
        assert!(event.fill.is_some());
        assert_eq!(event.avg_fill_price, Some(dec!(100.5)));
        Ok(())
    }

    #[test]
    fn missing_required_field_returns_mapping_error() -> anyhow::Result<()> {
        let result = BinanceOrderTradeUpdate::from_value(&json!({ "o": { "x": "NEW" } }));
        assert!(matches!(result, Err(DtoMappingError::MissingField("o.i"))));
        Ok(())
    }
}
