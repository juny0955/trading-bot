use async_trait::async_trait;

/// 텍스트 메시지를 외부 채널(텔레그램 등)로 전송하는 포트.
///
/// 구현체는 전송 실패 시 거래 루프에 영향을 주지 않아야 한다.
///
/// # Examples
///
/// ```ignore
/// # use std::sync::Arc;
/// # use trading_bot::ports::outbound::notification_sink::{NotificationSink, NoopNotificationSink};
/// let sink: Arc<dyn NotificationSink> = Arc::new(NoopNotificationSink);
/// sink.send("테스트 메시지".to_string()).await;
/// ```
#[async_trait]
pub trait NotificationSink: Send + Sync {
    /// 메시지를 전송한다.
    async fn send(&self, text: String);
}

/// 알림을 비활성화할 때 사용하는 no-op 구현체.
///
/// `bot_token` 설정이 없을 때 `TelegramNotifier` 대신 주입된다.
///
/// # Examples
///
/// ```ignore
/// let sink = NoopNotificationSink;
/// sink.send("무시됨".to_string()).await; // 아무 일도 하지 않음
/// ```
pub struct NoopNotificationSink;

#[async_trait]
impl NotificationSink for NoopNotificationSink {
    async fn send(&self, _text: String) {}
}
