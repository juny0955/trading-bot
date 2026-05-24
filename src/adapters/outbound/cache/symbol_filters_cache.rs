use crate::domain::exchange_info::SymbolFilters;
use crate::ports::exchange_info_source::{ExchangeInfoError, ExchangeInfoSource};
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

/// 심볼별 거래소 필터를 메모리에 캐싱하는 lock-free 컨테이너.
pub struct SymbolFiltersCache {
    inner: ArcSwap<HashMap<String, SymbolFilters>>,
}

impl SymbolFiltersCache {
    /// 빈 캐시를 생성한다.
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    /// 심볼에 해당하는 필터를 반환한다. 없으면 `None`.
    pub fn get(&self, symbol: &str) -> Option<SymbolFilters> {
        self.inner.load().get(symbol).cloned()
    }

    /// 캐시에 등록된 심볼 수를 반환한다.
    pub fn len(&self) -> usize {
        self.inner.load().len()
    }

    /// 캐시가 비어 있으면 `true`를 반환한다.
    pub fn is_empty(&self) -> bool {
        self.inner.load().is_empty()
    }

    /// 소스에서 전체 심볼 필터를 가져와 캐시를 원자적으로 교체한다.
    ///
    /// 성공 시 갱신된 심볼 수를 반환한다.
    pub async fn refresh<S: ExchangeInfoSource + ?Sized>(
        &self,
        src: &S,
    ) -> Result<usize, ExchangeInfoError> {
        let fresh: HashMap<String, SymbolFilters> = src
            .fetch_all()
            .await?
            .into_iter()
            .map(|f| (f.symbol.clone(), f))
            .collect();
        let n = fresh.len();
        self.inner.store(Arc::new(fresh));
        Ok(n)
    }
}

impl Default for SymbolFiltersCache {
    fn default() -> Self {
        Self::new()
    }
}
