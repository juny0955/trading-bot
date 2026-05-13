use std::time::Duration;

use tokio::{sync::mpsc::Sender, time::sleep};
use tracing::error;

use crate::dtos::{FngData, FngResponse};

const FALLBACK_INTERVAL_SECS: u64 = 3600;
const RETRY_INTERVAL_SECS: u64 = 60;

pub async fn fetch_alternative_fng(tx: Sender<FngData>) {
    let url = "https://api.alternative.me/fng/";
    loop {
        match reqwest::get(url).await {
            Ok(resp) => match resp.json::<FngResponse>().await {
                Ok(fng) => {
                    if let Some(data) = fng.data.into_iter().next() {
                        let interval = data
                            .time_until_update
                            .parse::<u64>()
                            .unwrap_or(FALLBACK_INTERVAL_SECS);

                        if tx.send(data).await.is_err() {
                            break;
                        }

                        sleep(Duration::from_secs(interval)).await;
                    }
                }
                Err(e) => {
                    error!("FNG 파싱 실패: {e}");
                    sleep(Duration::from_secs(RETRY_INTERVAL_SECS)).await;
                }
            },
            Err(e) => {
                error!("FNG 요청 실패: {e}");
                sleep(Duration::from_secs(RETRY_INTERVAL_SECS)).await;
            }
        }
    }
}
