#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    pub binance: BinanceRuntimeConfig,
    pub fng: FngRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct BinanceRuntimeConfig {
    pub reconnect_delay_sec: u64,
}

#[derive(Debug, Clone)]
pub struct FngRuntimeConfig {
    pub fallback_interval_sec: u64,
    pub retry_interval_sec: u64,
}

impl Default for BinanceRuntimeConfig {
    fn default() -> Self {
        Self {
            reconnect_delay_sec: 5,
        }
    }
}

impl Default for FngRuntimeConfig {
    fn default() -> Self {
        Self {
            fallback_interval_sec: 60,
            retry_interval_sec: 5,
        }
    }
}

impl RuntimeConfig {
    pub fn from_rows(rows: &[(String, String, String)]) -> Self {
        let mut cfg = Self::default();
        for (cfg_type, key, value) in rows {
            match (cfg_type.as_str(), key.as_str()) {
                ("binance", "reconnect_delay_sec") => {
                    if let Ok(v) = value.parse() {
                        cfg.binance.reconnect_delay_sec = v;
                    }
                }
                ("fng", "fallback_interval_sec") => {
                    if let Ok(v) = value.parse() {
                        cfg.fng.fallback_interval_sec = v;
                    }
                }
                ("fng", "retry_interval_sec") => {
                    if let Ok(v) = value.parse() {
                        cfg.fng.retry_interval_sec = v;
                    }
                }
                _ => {}
            }
        }
        cfg
    }
}
