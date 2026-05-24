use std::option::Option;
use std::time::Duration;

use crate::adapters::inbound::alternative::dto::{FngData, FngResponse};
use crate::config::AlternativeRuntimeConfig;
use crate::domain::event::MarketDataEvent;
use crate::ports::fng_provider::FngProvider;
use async_trait::async_trait;
use tokio::sync::watch::Sender;
use tokio::sync::{mpsc, watch};
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const URL: &str = "https://api.alternative.me/fng/";

/// Alternative.me 공포탐욕지수 provider입니다.
///
/// # Examples
///
/// ```ignore
/// # use trading_bot::adapters::inbound::alternative::alternative_fng_provider::AlternativeFngProvider;
/// # use trading_bot::config::AlternativeRuntimeConfig;
/// # fn example(cfg: AlternativeRuntimeConfig) {
/// let provider = AlternativeFngProvider::new(cfg);
/// # }
/// ```
pub struct AlternativeFngProvider {
    cfg: AlternativeRuntimeConfig,
}

impl AlternativeFngProvider {
    /// 새 Alternative.me provider를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// # use trading_bot::adapters::inbound::alternative::alternative_fng_provider::AlternativeFngProvider;
    /// # use trading_bot::config::AlternativeRuntimeConfig;
    /// # fn example(cfg: AlternativeRuntimeConfig) {
    /// let provider = AlternativeFngProvider::new(cfg);
    /// # }
    /// ```
    pub fn new(cfg: AlternativeRuntimeConfig) -> Self {
        Self { cfg }
    }
}

#[async_trait]
impl FngProvider for AlternativeFngProvider {
    async fn run(
        self: Box<Self>,
        market_tx: mpsc::Sender<MarketDataEvent>,
        token: CancellationToken,
    ) {
        let (tx, rx) = watch::channel::<Option<FngData>>(None);
        let fetch_token = token.clone();

        let fetch_handle = tokio::spawn(async move {
            fetch_alternative_fng(self.cfg, tx, fetch_token).await;
        });

        let mut rx = rx;
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                result = rx.changed() => match result {
                    Ok(()) => {
                        let Some(fng) = rx.borrow_and_update().clone() else { continue };
                        if let Err(e) = market_tx.send(fng.into()).await {
                            warn!(error = %e, "QuestDB Tx 채널 닫힘");
                            break;
                        }
                    }
                    Err(_) => break,
                }
            }
        }

        token.cancel();
        let _ = fetch_handle.await;
    }
}

/// Alternative.me API를 주기적으로 polling하고 watch channel로 전달합니다.
///
/// # Examples
///
/// ```ignore
/// # use tokio::sync::watch;
/// # use tokio_util::sync::CancellationToken;
/// # use trading_bot::config::AlternativeRuntimeConfig;
/// # async fn example(cfg: AlternativeRuntimeConfig) {
/// let (tx, _rx) = watch::channel(None);
/// trading_bot::adapters::inbound::alternative::alternative_fng_provider::fetch_alternative_fng(
///     cfg,
///     tx,
///     CancellationToken::new(),
/// ).await;
/// # }
/// ```
pub async fn fetch_alternative_fng(
    cfg: AlternativeRuntimeConfig,
    tx: Sender<Option<FngData>>,
    token: CancellationToken,
) {
    loop {
        let delay = tokio::select! {
            _ = token.cancelled() => break,
            d = fetch_once(&cfg, &tx) => match d {
                Some(d) => d,
                None => break,
            }
        };

        tokio::select! {
            _ = token.cancelled() => break,
            _ = sleep(delay) => {}
        }
    }
    info!("Alternative 수신 종료");
}

async fn fetch_once(
    cfg: &AlternativeRuntimeConfig,
    tx: &Sender<Option<FngData>>,
) -> Option<Duration> {
    let resp = match reqwest::get(URL).await {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "FNG 요청 실패");
            return Some(Duration::from_secs(cfg.retry_interval_sec));
        }
    };

    let fng = match resp.json::<FngResponse>().await {
        Ok(f) => f,
        Err(e) => {
            error!(error = %e, "FNG 파싱 실패");
            return Some(Duration::from_secs(cfg.retry_interval_sec));
        }
    };

    let Some(data) = fng.data.into_iter().next() else {
        warn!("FNG 응답에 데이터 없음");
        return Some(Duration::from_secs(cfg.retry_interval_sec));
    };

    let interval = data
        .time_until_update
        .parse::<u64>()
        .unwrap_or(cfg.fallback_interval_sec);

    tx.send_replace(Some(data));

    Some(Duration::from_secs(interval))
}
