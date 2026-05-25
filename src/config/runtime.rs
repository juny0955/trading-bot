#[derive(Debug, Clone, Default)]
pub struct RuntimeConfig {
    pub binance: BinanceRuntimeConfig,
    pub alternative: AlternativeRuntimeConfig,
    pub questdb: QuestDbRuntimeConfig,
    pub trading: TradingRuntimeConfig,
    pub telegram: TelegramRuntimeConfig,
}

/// 텔레그램 봇 설정. `bot_token`이 비어 있으면 알림이 비활성화된다.
#[derive(Debug, Clone, Default)]
pub struct TelegramRuntimeConfig {
    pub bot_token: String,
    pub chat_id: String,
}

#[derive(Debug, Clone, Default)]
pub struct TradingRuntimeConfig {
    /// mainnet 실거래 arming flag. false이면 주문 전송 불가.
    pub live_trading_enabled: bool,
    /// true이면 위험 검증만 수행하고 실제 주문 전송·DB 기록을 건너뜀.
    pub dry_run: bool,
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
    /// DB의 `runtime_config` 행들로부터 설정을 빌드한다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let rows = vec![
    ///     ("trading".into(), "dry_run".into(), "true".into()),
    ///     ("telegram".into(), "bot_token".into(), "abc".into()),
    /// ];
    /// let cfg = RuntimeConfig::from_rows(&rows);
    /// assert!(cfg.trading.dry_run);
    /// assert_eq!(cfg.telegram.bot_token, "abc");
    /// ```
    pub fn from_rows(rows: &[(String, String, String)]) -> Self {
        let mut cfg = Self::default();
        for (cfg_type, key, value) in rows {
            match cfg_type.as_str() {
                "binance" => cfg.binance_rows(key, value),
                "alternative" => cfg.alternative_rows(key, value),
                "questdb" => cfg.questdb_rows(key, value),
                "trading" => cfg.trading_rows(key, value),
                "telegram" => cfg.telegram_rows(key, value),
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

    fn trading_rows(&mut self, key: &str, value: &str) {
        match key {
            "live_trading_enabled" => parse(value, &mut self.trading.live_trading_enabled),
            "dry_run" => parse(value, &mut self.trading.dry_run),
            _ => {}
        }
    }

    fn telegram_rows(&mut self, key: &str, value: &str) {
        match key {
            "bot_token" => self.telegram.bot_token = value.to_string(),
            "chat_id" => self.telegram.chat_id = value.to_string(),
            _ => {}
        }
    }
}

fn parse<T: std::str::FromStr>(value: &str, target: &mut T) {
    if let Ok(v) = value.parse() {
        *target = v;
    }
}
