use crate::ports::outbound::notification_sink::NotificationSink;
use async_trait::async_trait;
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tracing::error;

/// Telegram Bot API를 통해 메시지를 전송하는 어댑터.
///
/// 전송 실패 시 `error!` 로그만 남기고 호출자에게 오류를 전파하지 않는다.
///
/// # Examples
///
/// ```ignore
/// let notifier = TelegramNotifier::new("123456:ABC-DEF", "-100123456789");
/// notifier.send("안녕하세요".to_string()).await;
/// ```
pub struct TelegramNotifier {
    client: Client,
    url: String,
    chat_id: String,
}

impl TelegramNotifier {
    /// 새 텔레그램 알리미를 생성한다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let notifier = TelegramNotifier::new("BOT_TOKEN", "CHAT_ID");
    /// ```
    pub fn new(bot_token: &str, chat_id: &str) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .expect("reqwest client 생성 실패");
        Self {
            client,
            url: sendmessage_url(bot_token),
            chat_id: chat_id.to_string(),
        }
    }
}

#[async_trait]
impl NotificationSink for TelegramNotifier {
    async fn send(&self, text: String) {
        let body = build_payload(&self.chat_id, &text);
        if let Err(e) = self.client.post(&self.url).json(&body).send().await {
            error!(error = %e, "텔레그램 메시지 전송 실패");
        }
    }
}

/// `https://api.telegram.org/bot{token}/sendMessage` URL을 조립한다.
fn sendmessage_url(bot_token: &str) -> String {
    format!("https://api.telegram.org/bot{bot_token}/sendMessage")
}

/// sendMessage 요청 바디를 JSON으로 빌드한다.
fn build_payload(chat_id: &str, text: &str) -> Value {
    json!({ "chat_id": chat_id, "text": text })
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;

    #[test]
    fn sendmessage_url_embeds_token() -> Result<()> {
        let url = sendmessage_url("mytoken123");
        assert!(url.contains("mytoken123"));
        assert!(url.ends_with("/sendMessage"));
        Ok(())
    }

    #[test]
    fn build_payload_sets_chat_id_and_text() -> Result<()> {
        let payload = build_payload("-100123", "hello");
        assert_eq!(payload["chat_id"], "-100123");
        assert_eq!(payload["text"], "hello");
        Ok(())
    }
}
