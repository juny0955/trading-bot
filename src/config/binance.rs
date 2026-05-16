use rusqlite::types::{FromSql, FromSqlResult, ValueRef};

#[derive(Debug, Clone)]
pub struct BinanceConfig {
    pub net: BinanceNet,
    pub symbols: Vec<SymbolConfig>,
    pub streams: Vec<StreamConfig>,
}

#[derive(Debug, Clone)]
pub enum BinanceNet {
    Testnet,
    Mainnet,
}

#[derive(Debug, Clone, PartialEq)]
pub enum StreamType {
    Public,
    Market,
    Private,
}

#[derive(Debug, Clone)]
pub struct SymbolConfig {
    pub symbol: String,
    pub enabled: bool,
}

#[derive(Debug, Clone)]
pub struct StreamConfig {
    pub name: String,
    pub stream_type: StreamType,
    pub suffix: String,
    pub enabled: bool,
}

impl FromSql for StreamType {
    fn column_result(value: ValueRef<'_>) -> FromSqlResult<Self> {
        match value.as_str()? {
            "MARKET" => Ok(StreamType::Market),
            "PUBLIC" => Ok(StreamType::Public),
            "PRIVATE" => Ok(StreamType::Private),
            _ => Err(rusqlite::types::FromSqlError::Other(
                format!("알수없는 stream_type: {}", value.as_str().unwrap_or("?")).into(),
            )),
        }
    }
}

impl BinanceConfig {
    pub fn public_ws_url(&self) -> String {
        let streams = self.build_streams(StreamType::Public);
        format!(
            "{}/public/stream?streams={}",
            self.base_url(),
            streams.join("/")
        )
    }

    pub fn market_ws_url(&self) -> String {
        let streams = self.build_streams(StreamType::Market);
        format!(
            "{}/market/stream?streams={}",
            self.base_url(),
            streams.join("/")
        )
    }

    fn base_url(&self) -> &str {
        match self.net {
            BinanceNet::Mainnet => "wss://fstream.binance.com",
            BinanceNet::Testnet => "wss://stream.binancefuture.com",
        }
    }

    fn build_streams(&self, stream_type: StreamType) -> Vec<String> {
        self.symbols
            .iter()
            .filter(|s| s.enabled)
            .flat_map(|s| {
                self.streams
                    .iter()
                    .filter(|st| st.enabled && st.stream_type == stream_type)
                    .map(move |st| format!("{}@{}", s.symbol.to_lowercase(), st.suffix))
            })
            .collect()
    }
}
