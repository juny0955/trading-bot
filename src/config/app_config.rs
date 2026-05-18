use crate::config::RuntimeConfig;
use crate::config::binance_config::BinanceConfig;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub binance: BinanceConfig,
    pub runtime: RuntimeConfig,
}
