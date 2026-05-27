use crate::domain::backtest::{Bar, DepthSnapshot};
use crate::ports::market_data_source::{BarQuery, DepthQuery, MarketDataSource};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

fn bar_interval_to_sample_by(i: crate::ports::market_data_source::BarInterval) -> String {
    use crate::ports::market_data_source::BarInterval;
    match i {
        BarInterval::Sec(n) => format!("{n}s"),
        BarInterval::Min(n) => format!("{n}m"),
        BarInterval::Hour(n) => format!("{n}h"),
    }
}

fn depth_interval_to_sample_by(i: crate::ports::market_data_source::DepthSampleInterval) -> String {
    use crate::ports::market_data_source::DepthSampleInterval;
    match i {
        DepthSampleInterval::Sec(n) => format!("{n}s"),
    }
}

/// QuestDB REST `/exec` API에서 백테스트용 시장 데이터를 조회하는 어댑터입니다.
///
/// # Examples
///
/// ```
/// use trading_bot::adapters::outbound::questdb_rest::data_source::QuestDbRestDataSource;
///
/// let source = QuestDbRestDataSource::new("http://localhost:9000");
/// let _ = source;
/// ```
pub struct QuestDbRestDataSource {
    base_url: String,
    client: Client,
}

impl QuestDbRestDataSource {
    /// 새 QuestDB REST data source를 생성합니다.
    ///
    /// # Examples
    ///
    /// ```
    /// use trading_bot::adapters::outbound::questdb_rest::data_source::QuestDbRestDataSource;
    ///
    /// let source = QuestDbRestDataSource::new("http://localhost:9000");
    /// let _ = source;
    /// ```
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            client: Client::new(),
        }
    }
}

#[derive(Deserialize)]
struct ExecResponse {
    dataset: Vec<Vec<serde_json::Value>>,
}

#[async_trait]
impl MarketDataSource for QuestDbRestDataSource {
    async fn fetch_bars(&self, q: BarQuery) -> Result<Vec<Bar>> {
        let sql = bars_sql(&q);
        let resp = self.exec(&sql).await?;
        resp.dataset
            .into_iter()
            .map(|row| parse_bar_row(row, &q.symbol))
            .collect()
    }

    async fn fetch_depth_snapshots(&self, q: DepthQuery) -> Result<Vec<DepthSnapshot>> {
        let sql = depth_sql(&q);
        let resp = self.exec(&sql).await?;
        resp.dataset
            .into_iter()
            .map(|row| parse_depth_row(row, &q.symbol))
            .collect()
    }
}

impl QuestDbRestDataSource {
    async fn exec(&self, sql: &str) -> Result<ExecResponse> {
        let url =
            reqwest::Url::parse_with_params(&format!("{}/exec", self.base_url), &[("query", sql)])
                .context("questdb url build 실패")?;
        let resp = self
            .client
            .get(url)
            .send()
            .await
            .context("questdb /exec 요청 실패")?;
        if !resp.status().is_success() {
            return Err(anyhow!(
                "questdb /exec status={}, body={}",
                resp.status(),
                resp.text().await.unwrap_or_default()
            ));
        }
        resp.json().await.context("questdb json 파싱 실패")
    }
}

fn bars_sql(q: &BarQuery) -> String {
    format!(
        "SELECT timestamp, first(price) o, max(price) h, min(price) l, \
        last(price) c, sum(quantity) v \
        FROM raw_trade \
        WHERE symbol = '{sym}' AND timestamp >= '{from}' AND timestamp < '{to}' \
        SAMPLE BY {interval} ALIGN TO CALENDAR",
        sym = escape_sql_literal(&q.symbol),
        from = ns_to_iso(q.from_ns),
        to = ns_to_iso(q.to_ns),
        interval = bar_interval_to_sample_by(q.interval),
    )
}

fn depth_sql(q: &DepthQuery) -> String {
    format!(
        "SELECT timestamp, first(bid1_price) bp, first(bid1_qty) bq, \
        first(ask1_price) ap, first(ask1_qty) aq \
        FROM orderbook_features_1m \
        WHERE symbol = '{sym}' AND timestamp >= '{from}' AND timestamp < '{to}' \
        SAMPLE BY {every} ALIGN TO CALENDAR",
        sym = escape_sql_literal(&q.symbol),
        from = ns_to_iso(q.from_ns),
        to = ns_to_iso(q.to_ns),
        every = depth_interval_to_sample_by(q.every),
    )
}

fn parse_bar_row(row: Vec<serde_json::Value>, symbol: &str) -> Result<Bar> {
    Ok(Bar {
        symbol: symbol.to_string(),
        ts_ns: row_ts_ns(&row, 0)?,
        open: row_decimal(&row, 1)?,
        high: row_decimal(&row, 2)?,
        low: row_decimal(&row, 3)?,
        close: row_decimal(&row, 4)?,
        volume: row_decimal(&row, 5)?,
    })
}

fn parse_depth_row(row: Vec<serde_json::Value>, symbol: &str) -> Result<DepthSnapshot> {
    Ok(DepthSnapshot {
        symbol: symbol.to_string(),
        ts_ns: row_ts_ns(&row, 0)?,
        bid1_price: row_decimal(&row, 1)?,
        bid1_qty: row_decimal(&row, 2)?,
        ask1_price: row_decimal(&row, 3)?,
        ask1_qty: row_decimal(&row, 4)?,
    })
}

fn row_ts_ns(row: &[serde_json::Value], index: usize) -> Result<i64> {
    parse_iso_to_ns(
        row.get(index)
            .and_then(serde_json::Value::as_str)
            .ok_or_else(|| anyhow!("timestamp column {index} is not string"))?,
    )
}

fn row_decimal(row: &[serde_json::Value], index: usize) -> Result<Decimal> {
    json_to_decimal(
        row.get(index)
            .ok_or_else(|| anyhow!("missing decimal column {index}"))?,
    )
}

fn escape_sql_literal(value: &str) -> String {
    value.replace('\'', "''")
}

fn ns_to_iso(ns: i64) -> String {
    let dt: DateTime<Utc> = DateTime::from_timestamp_nanos(ns);
    dt.format("%Y-%m-%dT%H:%M:%S%.6fZ").to_string()
}

fn parse_iso_to_ns(s: &str) -> Result<i64> {
    DateTime::parse_from_rfc3339(s)?
        .timestamp_nanos_opt()
        .ok_or_else(|| anyhow!("ts overflow"))
}

fn json_to_decimal(v: &serde_json::Value) -> Result<Decimal> {
    match v {
        serde_json::Value::Number(n) => Ok(Decimal::from_str(&n.to_string())?),
        serde_json::Value::String(s) => Ok(Decimal::from_str(s)?),
        _ => Err(anyhow!("expected number, got {v:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::market_data_source::{BarInterval, DepthSampleInterval};
    use rust_decimal_macros::dec;
    use serde_json::json;

    #[test]
    fn bars_sql_escapes_symbol_literal() -> anyhow::Result<()> {
        let sql = bars_sql(&BarQuery {
            symbol: "BTC'USDT".into(),
            from_ns: 0,
            to_ns: 1_000_000_000,
            interval: BarInterval::Min(1),
        });
        assert!(sql.contains("BTC''USDT"));
        assert!(sql.contains("SAMPLE BY 1m"));
        Ok(())
    }

    #[test]
    fn parse_bar_row_maps_dataset_columns() -> anyhow::Result<()> {
        let bar = parse_bar_row(
            vec![
                json!("1970-01-01T00:00:00.000000Z"),
                json!("1"),
                json!("2"),
                json!("0.5"),
                json!("1.5"),
                json!("10"),
            ],
            "BTCUSDT",
        )?;
        assert_eq!(bar.open, dec!(1));
        assert_eq!(bar.volume, dec!(10));
        Ok(())
    }

    #[test]
    fn depth_interval_formats_seconds() -> anyhow::Result<()> {
        assert_eq!(
            depth_interval_to_sample_by(DepthSampleInterval::Sec(5)),
            "5s"
        );
        Ok(())
    }
}
