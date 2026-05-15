use crate::config::{BinanceConfig, RuntimeConfig};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub binance: BinanceConfig,
    pub runtime: RuntimeConfig,
}
