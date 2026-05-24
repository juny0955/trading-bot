use crate::domain::exchange_info::SymbolFilters;
use crate::ports::exchange_info_source::{ExchangeInfoError, ExchangeInfoSource};
use arc_swap::ArcSwap;
use std::collections::HashMap;
use std::sync::Arc;

pub struct SymbolFiltersCache {
    inner: ArcSwap<HashMap<String, SymbolFilters>>,
}

impl SymbolFiltersCache {
    pub fn new() -> Self {
        Self {
            inner: ArcSwap::from_pointee(HashMap::new()),
        }
    }

    pub fn get(&self, symbol: &str) -> Option<SymbolFilters> {
        self.inner.load().get(symbol).cloned()
    }

    pub fn len(&self) -> usize {
        self.inner.load().len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.load().is_empty()
    }

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
