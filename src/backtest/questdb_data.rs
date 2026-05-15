use crate::backtest::data::{BarQuery, DepthQuery, MarketDataSource};
use crate::backtest::types::{Bar, DepthSnapshot};
use anyhow::{Context, Result, anyhow};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use reqwest::Client;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::str::FromStr;

pub struct QuestDbRestDataSource {
    base_url: String,
    client: Client,
}

impl QuestDbRestDataSource {
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
        let from_iso = ns_to_iso(q.from_ns);
        let to_iso = ns_to_iso(q.to_ns);
        let sql = format!(
            "SELECT timestamp, first(price) o, max(price) h, min(price) l, \
            last(price) c, sum(quantity) v \
            FROM trades \
            WHERE symbol = '{sym}' AND timestamp >= '{from}' AND timestamp < '{to}' \
            SAMPLE BY {interval} ALIGN TO CALENDAR",
            sym = q.symbol,
            from = from_iso,
            to = to_iso,
            interval = q.interval,
        );

        let resp = self.exec(&sql).await?;
        let mut out = Vec::with_capacity(resp.dataset.len());
        for row in resp.dataset {
            let ts = parse_iso_to_ns(row[0].as_str().ok_or_else(|| anyhow!("ts not str"))?)?;
            let o = json_to_decimal(&row[1])?;
            let h = json_to_decimal(&row[2])?;
            let l = json_to_decimal(&row[3])?;
            let c = json_to_decimal(&row[4])?;
            let v = json_to_decimal(&row[5])?;
            out.push(Bar {
                symbol: q.symbol.clone(),
                ts_ns: ts,
                open: o,
                high: h,
                low: l,
                close: c,
                volume: v,
            });
        }
        Ok(out)
    }

    async fn fetch_depth_snapshots(&self, q: DepthQuery) -> anyhow::Result<Vec<DepthSnapshot>> {
        let from_iso = ns_to_iso(q.from_ns);
        let to_iso = ns_to_iso(q.to_ns);
        let sql = format!(
            "SELECT timestamp, first(bid1_price) bp, first(bid1_qty) bq, \
              first(ask1_price) ap, first(ask1_qty) aq \
              FROM depth \
              WHERE symbol = '{sym}' AND timestamp >= '{from}' AND timestamp < '{to}' \
              SAMPLE BY {every} ALIGN TO CALENDAR",
            sym = q.symbol,
            from = from_iso,
            to = to_iso,
            every = q.every,
        );

        let resp = self.exec(&sql).await?;
        let mut out = Vec::with_capacity(resp.dataset.len());
        for row in resp.dataset {
            let ts = parse_iso_to_ns(row[0].as_str().ok_or_else(|| anyhow!("ts not str"))?)?;
            let bp = json_to_decimal(&row[1])?;
            let bq = json_to_decimal(&row[2])?;
            let ap = json_to_decimal(&row[3])?;
            let aq = json_to_decimal(&row[4])?;
            out.push(DepthSnapshot {
                symbol: q.symbol.clone(),
                ts_ns: ts,
                bid1_price: bp,
                bid1_qty: bq,
                ask1_price: ap,
                ask1_qty: aq,
            });
        }
        Ok(out)
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
