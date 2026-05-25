use crate::ports::outbound::notification_sink::NotificationSink;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::json;
use std::time::Duration;
use tracing::error;

/// Telegram Bot API를 통해 메시지를 전송하는 어댑터.
///
/// 전송 실패 시 `error!` 로그만 남기고 호출자에게 오류를 전파하지 않는다.
pub struct TelegramNotifier {
    client: Client,
    url: String,
    chat_id: String,
}

impl TelegramNotifier {
    /// 새 텔레그램 알리미를 생성한다.
    pub fn new(bot_token: &str, chat_id: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client 생성 실패");
        let url = format!("https://api.telegram.org/bot{bot_token}/sendMessage");
        Self {
            client,
            url,
            chat_id: chat_id.to_string(),
        }
    }
}

#[async_trait]
impl NotificationSink for TelegramNotifier {
    async fn send(&self, text: String) {
        let body = json!({ "chat_id": self.chat_id, "text": text });
        if let Err(e) = self.client.post(&self.url).json(&body).send().await {
            error!(error = %e, "텔레그램 메시지 전송 실패");
        }
    }
}
