use crate::domain::market_data::BookTicker;
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

pub struct BookTickerCache {
    inner: ArcSwap<HashMap<String, BookTicker>>,
}

impl BookTickerCache {
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    pub fn get(&self, symbol: &str) -> Option<BookTicker> {
        self.inner.load().get(symbol).cloned()
    }

    /// 단일 심볼 업데이트 — 통째 교체.
    /// 호출 빈도가 매우 높을 수 있어 HashMap clone 비용 ≒ 심볼 수 × O(1).
    /// 일반 운용(< 50 심볼)에서는 미미. 그 이상이면 DashMap으로 교체 고려.
    pub fn update(&self, bt: BookTicker) {
        let mut next = (**self.inner.load()).clone();
        next.insert(bt.symbol.clone(), bt);
        self.inner.store(Arc::new(next));
    }

    pub fn snapshot(&self) -> Arc<HashMap<String, BookTicker>> {
        self.inner.load_full()
    }
}

impl Default for BookTickerCache {
    fn default() -> Self {
        Self::new()
    }
}
