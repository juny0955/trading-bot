use async_trait::async_trait;

/// 텍스트 메시지를 외부 채널(텔레그램 등)로 전송하는 포트.
#[async_trait]
pub trait NotificationSink: Send + Sync {
    /// 메시지를 전송한다. 실패해도 거래 루프에 영향을 주지 않아야 한다.
    async fn send(&self, text: String);
}

/// 알림을 비활성화할 때 사용하는 아무 작업도 하지 않는 구현체.
pub struct NoopNotificationSink;

#[async_trait]
impl NotificationSink for NoopNotificationSink {
    async fn send(&self, _text: String) {}
}
