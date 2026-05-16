use std::time::Duration;

use crate::config::AlternativeRuntimeConfig;
use crate::market_data::alternative::dto::{FngData, FngResponse};
use tokio::{sync::mpsc::Sender, time::sleep};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const URL: &str = "https://api.alternative.me/fng/";

pub async fn fetch_alternative_fng(
    cfg: AlternativeRuntimeConfig,
    tx: Sender<FngData>,
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

async fn fetch_once(cfg: &AlternativeRuntimeConfig, tx: &Sender<FngData>) -> Option<Duration> {
    let resp = match reqwest::get(URL).await {
        Ok(r) => r,
        Err(e) => {
            error!("FNG 요청 실패: {e}");
            return Some(Duration::from_secs(cfg.retry_interval_sec));
        }
    };

    let fng = match resp.json::<FngResponse>().await {
        Ok(f) => f,
        Err(e) => {
            error!("FNG 파싱 실패: {e}");
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

    if let Err(e) = tx.send(data).await {
        warn!("FNG 채널 닫힘: {e}");
        return None;
    }

    Some(Duration::from_secs(interval))
}
