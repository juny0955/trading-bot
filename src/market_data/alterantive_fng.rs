use std::time::Duration;

use tokio::{sync::mpsc::Sender, time::sleep};
use tracing::error;

use crate::{
    FngRuntimeConfig,
    dtos::{FngData, FngResponse},
};

const URL: &str = "https://api.alternative.me/fng/";

pub async fn fetch_alternative_fng(cfg: FngRuntimeConfig, tx: Sender<FngData>) {
    loop {
        match reqwest::get(URL).await {
            Ok(resp) => match resp.json::<FngResponse>().await {
                Ok(fng) => {
                    if let Some(data) = fng.data.into_iter().next() {
                        let interval = data
                            .time_until_update
                            .parse::<u64>()
                            .unwrap_or(cfg.fallback_interval_sec);

                        if tx.send(data).await.is_err() {
                            break;
                        }

                        sleep(Duration::from_secs(interval)).await;
                    }
                }
                Err(e) => {
                    error!("FNG 파싱 실패: {e}");
                    sleep(Duration::from_secs(cfg.retry_interval_sec)).await;
                }
            },
            Err(e) => {
                error!("FNG 요청 실패: {e}");
                sleep(Duration::from_secs(cfg.retry_interval_sec)).await;
            }
        }
    }
}
