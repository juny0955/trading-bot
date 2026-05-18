use crate::{binance::BinanceConfig, config::RuntimeConfig};

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub binance: BinanceConfig,
    pub runtime: RuntimeConfig,
}
