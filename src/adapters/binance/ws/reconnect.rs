use async_trait::async_trait;
use std::time::Duration;
use tokio::time::sleep;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// WebSocket 세션 실행 결과의 재연결 정책을 표현합니다.
///
/// # Examples
///
/// ```
/// use trading_bot::adapters::binance::ws::reconnect::WsError;
///
/// let err = WsError::retryable(anyhow::anyhow!("temporary"));
/// assert!(err.to_string().contains("retryable"));
/// ```
#[derive(Debug, thiserror::Error)]
pub enum WsError {
    #[error("retryable: {0:#}")]
    Retryable(#[source] anyhow::Error),
    #[error("fatal: {0:#}")]
    Fatal(#[source] anyhow::Error),
}

impl WsError {
    /// 재시도 가능한 WebSocket 오류를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::adapters::binance::ws::reconnect::WsError;
    ///
    /// let err = WsError::retryable(anyhow::anyhow!("timeout"));
    /// assert!(err.to_string().contains("retryable"));
    /// ```
    pub fn retryable(e: impl Into<anyhow::Error>) -> Self {
        Self::Retryable(e.into())
    }

    /// 재시도하지 않을 WebSocket 오류를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::adapters::binance::ws::reconnect::WsError;
    ///
    /// let err = WsError::fatal(anyhow::anyhow!("auth"));
    /// assert!(err.to_string().contains("fatal"));
    /// ```
    pub fn fatal(e: impl Into<anyhow::Error>) -> Self {
        Self::Fatal(e.into())
    }
}

/// WebSocket 재연결 backoff 설정입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::adapters::binance::ws::reconnect::ReconnectConfig;
///
/// let cfg = ReconnectConfig::defaults("market");
/// assert_eq!(cfg.label, "market");
/// ```
pub struct ReconnectConfig {
    pub label: String,
    pub initial_delay: Duration,
    pub max_delay: Duration,
    pub backoff_multiplier: f64,
    pub max_retries: Option<u32>, // None=무한, Some(N)=상한
}

impl ReconnectConfig {
    /// 기본 재연결 설정을 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::adapters::binance::ws::reconnect::ReconnectConfig;
    ///
    /// let cfg = ReconnectConfig::defaults("user");
    /// assert!(cfg.max_retries.is_none());
    /// ```
    pub fn defaults(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 2.0,
            max_retries: None,
        }
    }
}

/// 재연결 루프에서 실행할 WebSocket 세션 인터페이스입니다.
///
/// # Examples
///
/// ```ignore
/// # use async_trait::async_trait;
/// # use tokio_util::sync::CancellationToken;
/// # use trading_bot::adapters::binance::ws::reconnect::{WsError, WsSession};
/// struct Session;
///
/// #[async_trait]
/// impl WsSession for Session {
///     async fn run_once(&mut self, token: CancellationToken) -> Result<(), WsError> {
///         Ok(())
///     }
/// }
/// ```
#[async_trait]
pub trait WsSession: Send {
    async fn run_once(&mut self, token: CancellationToken) -> Result<(), WsError>;
    async fn on_disconnect(&mut self) {}
}

/// `WsSession`을 재연결 정책에 따라 반복 실행합니다.
///
/// # Examples
///
/// ```ignore
/// # async fn example(session: impl trading_bot::adapters::binance::ws::reconnect::WsSession) {
/// use tokio_util::sync::CancellationToken;
/// use trading_bot::adapters::binance::ws::reconnect::{ReconnectConfig, run_with_reconnect};
///
/// run_with_reconnect(session, ReconnectConfig::defaults("ws"), CancellationToken::new()).await;
/// # }
/// ```
pub async fn run_with_reconnect<S: WsSession>(
    mut session: S,
    cfg: ReconnectConfig,
    token: CancellationToken,
) {
    let mut attempt: u32 = 0;
    let mut delay = cfg.initial_delay;

    loop {
        tokio::select! {
            _ = token.cancelled() => {
                info!(label = %cfg.label, "WS 종료");
                return;
            }
            res = session.run_once(token.clone()) => match res {
                Ok(()) => return,
                Err(WsError::Fatal(e)) => {
                    error!(label = %cfg.label, error = %e, "재연결 중단");
                    session.on_disconnect().await;
                    return;
                }
                Err(WsError::Retryable(e)) => {
                    session.on_disconnect().await;
                    attempt += 1;
                    if let Some(limit) = cfg.max_retries && attempt > limit {
                        error!(label = %cfg.label, attempt = %attempt, limit = %limit, error = %e, "재시도 초과");
                        return;
                    }

                    warn!(label = %cfg.label, error = %e, delay_sec = delay.as_secs(), "재연결 예정");
                    tokio::select! {
                        _ = token.cancelled() => return,
                        _ = sleep(delay) => {}
                    }
                    let next = delay.mul_f64(cfg.backoff_multiplier);
                    delay = if next > cfg.max_delay { cfg.max_delay } else { next };
                }
            }
        }
    }
}
