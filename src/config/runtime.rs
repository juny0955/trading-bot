#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    pub binance: BinanceRuntimeConfig,
    pub alternative: AlternativeRuntimeConfig,
    pub questdb: QuestDbRuntimeConfig,
}

#[derive(Debug, Clone)]
pub struct BinanceRuntimeConfig {
    pub reconnect_delay_sec: u64,
    pub read_timeout_sec: u64,
}

#[derive(Debug, Clone)]
pub struct AlternativeRuntimeConfig {
    pub fallback_interval_sec: u64,
    pub retry_interval_sec: u64,
}

#[derive(Debug, Clone)]
pub struct QuestDbRuntimeConfig {
    pub batch_max_rows: usize,
    pub batch_interval_ms: u64,
    pub buffer_max_bytes: usize,
}

impl Default for BinanceRuntimeConfig {
    fn default() -> Self {
        Self {
            reconnect_delay_sec: 5,
            read_timeout_sec: 60,
        }
    }
}

impl Default for AlternativeRuntimeConfig {
    fn default() -> Self {
        Self {
            fallback_interval_sec: 60,
            retry_interval_sec: 5,
        }
    }
}

impl Default for QuestDbRuntimeConfig {
    fn default() -> Self {
        Self {
            batch_max_rows: 5_000,
            batch_interval_ms: 100,
            buffer_max_bytes: 1_048_576,
        }
    }
}

impl RuntimeConfig {
    pub fn from_rows(rows: &[(String, String, String)]) -> Self {
        let mut cfg = Self::default();
        for (cfg_type, key, value) in rows {
            match cfg_type.as_str() {
                "binance" => cfg.binance_rows(key, value),
                "alternative" => cfg.alternative_rows(key, value),
                "questdb" => cfg.questdb_rows(key, value),
                _ => {}
            }
        }
        cfg
    }

    fn binance_rows(&mut self, key: &str, value: &str) {
        match key {
            "reconnect_delay_sec" => parse(value, &mut self.binance.reconnect_delay_sec),
            "read_timeout_sec" => parse(value, &mut self.binance.read_timeout_sec),
            _ => {}
        }
    }

    fn alternative_rows(&mut self, key: &str, value: &str) {
        match key {
            "fallback_interval_sec" => parse(value, &mut self.alternative.fallback_interval_sec),
            "retry_interval_sec" => parse(value, &mut self.alternative.retry_interval_sec),
            _ => {}
        }
    }

    fn questdb_rows(&mut self, key: &str, value: &str) {
        match key {
            "batch_max_rows" => parse(value, &mut self.questdb.batch_max_rows),
            "batch_interval_ms" => parse(value, &mut self.questdb.batch_interval_ms),
            "buffer_max_bytes" => parse(value, &mut self.questdb.buffer_max_bytes),
            _ => {}
        }
    }
}

fn parse<T: std::str::FromStr>(value: &str, target: &mut T) {
    if let Ok(v) = value.parse() {
        *target = v;
    }
}
