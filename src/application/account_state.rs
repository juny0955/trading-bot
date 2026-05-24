use crate::domain::account::{AccountSnapshot, AccountUpdateEvent};
use arc_swap::ArcSwap;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::debug;

/// arc-swap 기반의 lock-free 계정 상태 컨테이너.
///
/// REST refresher와 user-stream task가 동시에 갱신하더라도
/// `fetched_at`이 더 최신인 쪽만 살아남음 (compare_and_swap 루프 = optimistic concurrency).
pub struct AccountState {
    inner: ArcSwap<AccountSnapshot>,
}

impl AccountState {
    /// 초기 스냅샷으로 `AccountState`를 생성한다.
    ///
    /// # Examples
    ///
    /// ```ignore
    /// let state = AccountState::new(AccountSnapshot::default());
    /// assert_eq!(state.available_balance(), Decimal::ZERO);
    /// ```
    pub fn new(initial: AccountSnapshot) -> Self {
        Self {
            inner: ArcSwap::from_pointee(initial),
        }
    }

    /// 현재 스냅샷을 lock-free로 읽는다.
    pub fn snapshot(&self) -> Arc<AccountSnapshot> {
        self.inner.load_full()
    }

    /// 가용 잔고를 lock-free로 읽는다.
    pub fn available_balance(&self) -> Decimal {
        self.inner.load().available_balance
    }

    /// stale write 방지 CAS — `fetched_at` 기준 최신인 경우만 반영.
    ///
    /// REST polling이 늦게 도착해 user-stream 최신값을 덮어쓰는 race를 방지.
    pub fn try_replace(&self, candidate: AccountSnapshot) -> bool {
        loop {
            let current = self.inner.load_full();
            if candidate.fetched_at <= current.fetched_at {
                return false; // stale → 무시
            }
            let prev = self.inner.compare_and_swap(&current, Arc::new(candidate.clone()));
            if Arc::ptr_eq(&prev, &current) {
                return true;
            }
        }
    }

    /// user-stream의 ACCOUNT_UPDATE 이벤트를 반영.
    /// 거래소가 eventTime을 보장하므로 REST polling 결과보다 항상 우선.
    pub fn apply_account_update(&self, ev: &AccountUpdateEvent) {
        let next = AccountSnapshot {
            available_balance: ev.available_balance,
            total_wallet_balance: ev.total_wallet_balance,
            total_unrealized_pnl: ev.total_unrealized_pnl,
            fetched_at: ev.event_time,
        };
        let replaced = self.try_replace(next);
        debug!(replaced, "account_update 적용");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Utc};
    use rust_decimal_macros::dec;

    fn snap(balance: Decimal, offset_ms: i64) -> AccountSnapshot {
        AccountSnapshot {
            available_balance: balance,
            total_wallet_balance: balance,
            total_unrealized_pnl: dec!(0),
            fetched_at: Utc::now() + Duration::milliseconds(offset_ms),
        }
    }

    #[test]
    fn newer_snapshot_replaces_older() {
        let state = AccountState::new(snap(dec!(100), 0));
        assert!(state.try_replace(snap(dec!(200), 1000)));
        assert_eq!(state.available_balance(), dec!(200));
    }

    #[test]
    fn stale_snapshot_is_ignored() {
        let state = AccountState::new(snap(dec!(200), 1000));
        assert!(!state.try_replace(snap(dec!(100), 0)));
        assert_eq!(state.available_balance(), dec!(200));
    }

    #[test]
    fn same_timestamp_is_ignored() {
        let now = Utc::now();
        let s = AccountSnapshot {
            available_balance: dec!(200),
            total_wallet_balance: dec!(200),
            total_unrealized_pnl: dec!(0),
            fetched_at: now,
        };
        let state = AccountState::new(s.clone());
        assert!(!state.try_replace(s));
        assert_eq!(state.available_balance(), dec!(200));
    }
}
