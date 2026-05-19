use crate::domain::order::{OrderError, OrderSide, OrderStatus, OrderType, TimeInForce};
use sqlx::Postgres;
use sqlx::encode::IsNull;
use sqlx::postgres::{PgArgumentBuffer, PgTypeInfo, PgValueRef};

type BoxDynError = Box<dyn std::error::Error + Send + Sync + 'static>;

impl From<sqlx::Error> for OrderError {
    fn from(e: sqlx::Error) -> Self {
        OrderError::Storage(e.to_string())
    }
}

// ── OrderType ────────────────────────────────────────────────────────────────

impl sqlx::Type<Postgres> for OrderType {
    fn type_info() -> PgTypeInfo {
        <str as sqlx::Type<Postgres>>::type_info()
    }
    fn compatible(ty: &PgTypeInfo) -> bool {
        <str as sqlx::Type<Postgres>>::compatible(ty)
    }
}
impl<'r> sqlx::Decode<'r, Postgres> for OrderType {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match <&str as sqlx::Decode<Postgres>>::decode(value)? {
            "market" => Ok(Self::Market),
            "limit" => Ok(Self::Limit),
            "stop_market" => Ok(Self::StopMarket),
            "stop_limit" => Ok(Self::StopLimit),
            other => Err(format!("unknown OrderType: {other}").into()),
        }
    }
}
impl sqlx::Encode<'_, Postgres> for OrderType {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let s = match self {
            Self::Market => "market",
            Self::Limit => "limit",
            Self::StopMarket => "stop_market",
            Self::StopLimit => "stop_limit",
        };
        <&str as sqlx::Encode<Postgres>>::encode_by_ref(&s, buf)
    }
}

// ── OrderSide ────────────────────────────────────────────────────────────────

impl sqlx::Type<Postgres> for OrderSide {
    fn type_info() -> PgTypeInfo {
        <str as sqlx::Type<Postgres>>::type_info()
    }
    fn compatible(ty: &PgTypeInfo) -> bool {
        <str as sqlx::Type<Postgres>>::compatible(ty)
    }
}
impl<'r> sqlx::Decode<'r, Postgres> for OrderSide {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match <&str as sqlx::Decode<Postgres>>::decode(value)? {
            "buy" => Ok(Self::Buy),
            "sell" => Ok(Self::Sell),
            other => Err(format!("unknown OrderSide: {other}").into()),
        }
    }
}
impl sqlx::Encode<'_, Postgres> for OrderSide {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let s = match self {
            Self::Buy => "buy",
            Self::Sell => "sell",
        };
        <&str as sqlx::Encode<Postgres>>::encode_by_ref(&s, buf)
    }
}

// ── OrderStatus ──────────────────────────────────────────────────────────────

impl sqlx::Type<Postgres> for OrderStatus {
    fn type_info() -> PgTypeInfo {
        <str as sqlx::Type<Postgres>>::type_info()
    }
    fn compatible(ty: &PgTypeInfo) -> bool {
        <str as sqlx::Type<Postgres>>::compatible(ty)
    }
}
impl<'r> sqlx::Decode<'r, Postgres> for OrderStatus {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match <&str as sqlx::Decode<Postgres>>::decode(value)? {
            "new" => Ok(Self::New),
            "partially_filled" => Ok(Self::PartiallyFilled),
            "filled" => Ok(Self::Filled),
            "cancelled" => Ok(Self::Cancelled),
            "rejected" => Ok(Self::Rejected),
            "expired" => Ok(Self::Expired),
            other => Err(format!("unknown OrderStatus: {other}").into()),
        }
    }
}
impl sqlx::Encode<'_, Postgres> for OrderStatus {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let s = match self {
            Self::New => "new",
            Self::PartiallyFilled => "partially_filled",
            Self::Filled => "filled",
            Self::Cancelled => "cancelled",
            Self::Rejected => "rejected",
            Self::Expired => "expired",
        };
        <&str as sqlx::Encode<Postgres>>::encode_by_ref(&s, buf)
    }
}

// ── TimeInForce ──────────────────────────────────────────────────────────────

impl sqlx::Type<Postgres> for TimeInForce {
    fn type_info() -> PgTypeInfo {
        <str as sqlx::Type<Postgres>>::type_info()
    }
    fn compatible(ty: &PgTypeInfo) -> bool {
        <str as sqlx::Type<Postgres>>::compatible(ty)
    }
}
impl<'r> sqlx::Decode<'r, Postgres> for TimeInForce {
    fn decode(value: PgValueRef<'r>) -> Result<Self, BoxDynError> {
        match <&str as sqlx::Decode<Postgres>>::decode(value)? {
            "gtc" => Ok(Self::Gtc),
            "ioc" => Ok(Self::Ioc),
            "fok" => Ok(Self::Fok),
            other => Err(format!("unknown TimeInForce: {other}").into()),
        }
    }
}
impl sqlx::Encode<'_, Postgres> for TimeInForce {
    fn encode_by_ref(&self, buf: &mut PgArgumentBuffer) -> Result<IsNull, BoxDynError> {
        let s = match self {
            Self::Gtc => "gtc",
            Self::Ioc => "ioc",
            Self::Fok => "fok",
        };
        <&str as sqlx::Encode<Postgres>>::encode_by_ref(&s, buf)
    }
}
