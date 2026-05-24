use crate::domain::market_data::BookTicker;
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

/// 심볼별 최신 호가를 메모리에 캐싱하는 lock-free 컨테이너.
pub struct BookTickerCache {
    inner: ArcSwap<HashMap<String, BookTicker>>,
}

impl BookTickerCache {
    /// 빈 캐시를 생성한다.
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    /// 심볼에 해당하는 호가를 반환한다. 없으면 `None`.
    pub fn get(&self, symbol: &str) -> Option<BookTicker> {
        self.inner.load().get(symbol).cloned()
    }

    /// 단일 심볼 호가를 갱신한다 (HashMap 통째 교체).
    ///
    /// 심볼 수가 50개 미만이면 교체 비용이 미미하다. 그 이상이면 DashMap으로 교체를 고려.
    pub fn update(&self, bt: BookTicker) {
        let mut next = (**self.inner.load()).clone();
        next.insert(bt.symbol.clone(), bt);
        self.inner.store(Arc::new(next));
    }

    /// 전체 캐시 스냅샷을 반환한다.
    pub fn snapshot(&self) -> Arc<HashMap<String, BookTicker>> {
        self.inner.load_full()
    }
}

impl Default for BookTickerCache {
    fn default() -> Self {
        Self::new()
    }
}
