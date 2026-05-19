use rust_decimal::Decimal;
use serde_json::Value;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum DtoMappingError {
    #[error("missing field: {0}")]
    MissingField(&'static str),
    #[error("parse error: {0}")]
    Parse(String),
}

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
    pub fn from_value(val: &Value) -> Result<Self, DtoMappingError> {
        let o = &val["o"];
        Ok(Self {
            exchange_order_id: o["i"]
                .as_i64()
                .ok_or(DtoMappingError::MissingField("o.i"))?,
            execution_type: o["x"]
                .as_str()
                .ok_or(DtoMappingError::MissingField("o.x"))?
                .to_string(),
            status: o["X"]
                .as_str()
                .ok_or(DtoMappingError::MissingField("o.X"))?
                .to_string(),
            side: o["S"]
                .as_str()
                .ok_or(DtoMappingError::MissingField("o.S"))?
                .to_string(),
            last_filled_qty: o["l"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .map_err(|e| DtoMappingError::Parse(format!("o.l: {e}")))?,
            last_fill_price: o["L"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .map_err(|e| DtoMappingError::Parse(format!("o.L: {e}")))?,
            fee: o["n"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .map_err(|e| DtoMappingError::Parse(format!("o.n: {e}")))?,
            fee_asset: o["N"].as_str().unwrap_or("USDT").to_string(),
            trade_time_ms: o["T"].as_i64().unwrap_or(0),
            cumulative_filled_qty: o["z"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .map_err(|e| DtoMappingError::Parse(format!("o.z: {e}")))?,
            avg_fill_price: o["ap"]
                .as_str()
                .unwrap_or("0")
                .parse()
                .map_err(|e| DtoMappingError::Parse(format!("o.ap: {e}")))?,
        })
    }
}
