use crate::domain::backtest::{Bar, DepthSnapshot};
use anyhow::Result;
use async_trait::async_trait;

#[derive(Debug, Clone, Copy)]
pub enum BarInterval {
    Sec(u32),
    Min(u32),
    Hour(u32),
}

impl std::str::FromStr for BarInterval {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (n, unit) = s
            .find(|c: char| c.is_alphabetic())
            .map(|i| (&s[..i], &s[i..]))
            .ok_or_else(|| format!("잘못된 BarInterval: {s}"))?;
        let n: u32 = n.parse().map_err(|_| format!("숫자 파싱 실패: {n}"))?;
        match unit {
            "s" => Ok(BarInterval::Sec(n)),
            "m" => Ok(BarInterval::Min(n)),
            "h" => Ok(BarInterval::Hour(n)),
            other => Err(format!("알 수 없는 단위: {other}")),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub enum DepthSampleInterval {
    Sec(u32),
}

impl std::str::FromStr for DepthSampleInterval {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (n, unit) = s
            .find(|c: char| c.is_alphabetic())
            .map(|i| (&s[..i], &s[i..]))
            .ok_or_else(|| format!("잘못된 DepthSampleInterval: {s}"))?;
        let n: u32 = n.parse().map_err(|_| format!("숫자 파싱 실패: {n}"))?;
        match unit {
            "s" => Ok(DepthSampleInterval::Sec(n)),
            other => Err(format!("DepthSampleInterval은 초(s)만 지원: {other}")),
        }
    }
}

#[async_trait]
pub trait MarketDataSource: Send + Sync {
    async fn fetch_bars(&self, q: BarQuery) -> Result<Vec<Bar>>;
    async fn fetch_depth_snapshots(&self, q: DepthQuery) -> Result<Vec<DepthSnapshot>>;
}

pub struct BarQuery {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub interval: BarInterval, // "1m", "5m" 등 QuestDB SAMPLE BY 문법
}

pub struct DepthQuery {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub every: DepthSampleInterval, // "5s" 등 샘플링 주기
}
