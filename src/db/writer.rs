use questdb::ingress::{Buffer, ProtocolVersion, TimestampNanos};
use rust_decimal::prelude::ToPrimitive;
use std::f64;
use tokio::sync::mpsc::Receiver;
use tracing::error;

use crate::dtos::{BookTickerData, DepthData, FngData, TradeData};

pub enum DbEvent {
    Trade(TradeData),
    Depth(DepthData),
    BookTicker(BookTickerData),
    Fng(FngData),
}
pub async fn run(conf: &str, mut rx: Receiver<DbEvent>) {
    let mut sender = questdb::ingress::Sender::from_conf(conf).expect("QuestDB 연결 실패");

    let mut buf = Buffer::new(ProtocolVersion::V2);
    while let Some(event) = rx.recv().await {
        let result = match &event {
            DbEvent::Trade(d) => append_trade(&mut buf, d),
            DbEvent::Depth(d) => append_depth(&mut buf, d),
            DbEvent::BookTicker(d) => append_book_ticker(&mut buf, d),
            DbEvent::Fng(d) => append_fng(&mut buf, d),
        };

        if result.is_ok()
            && let Err(e) = sender.flush(&mut buf)
        {
            error!("QuestDB flush 실패: {e}");
        }
    }
}

fn append_trade(buf: &mut Buffer, d: &TradeData) -> questdb::Result<()> {
    buf.table("trades")?
        .symbol("symbol", &d.symbol)?
        .column_f64("price", d.price.to_f64().unwrap_or(f64::NAN))?
        .column_f64("quantity", d.quantity.to_f64().unwrap_or(f64::NAN))?
        .column_bool("buyer_is_market_maker", d.buyer_is_market_maker)?
        .at(TimestampNanos::new(d.time as i64 * 1_000_000))?;

    Ok(())
}

fn append_depth(buf: &mut Buffer, d: &DepthData) -> questdb::Result<()> {
    let row = buf.table("depth")?;
    row.symbol("symbol", &d.symbol)?
        .column_i64("first_update_id", d.first_update_id as i64)?
        .column_i64("last_update_id", d.last_update_id as i64)?;

    // 매수(Bids) 10단계 처리
    for i in 0..10 {
        let (price, qty) = d
            .bids
            .get(i)
            .map(|p| (p.0.to_f64().unwrap_or(0.0), p.1.to_f64().unwrap_or(0.0)))
            .unwrap_or((0.0, 0.0));

        row.column_f64(format!("bid{}_price", i + 1).as_str(), price)?
            .column_f64(format!("bid{}_qty", i + 1).as_str(), qty)?;
    }

    // 매도(Asks) 10단계 처리
    for i in 0..10 {
        let (price, qty) = d
            .asks
            .get(i)
            .map(|p| (p.0.to_f64().unwrap_or(0.0), p.1.to_f64().unwrap_or(0.0)))
            .unwrap_or((0.0, 0.0));

        row.column_f64(format!("ask{}_price", i + 1).as_str(), price)?
            .column_f64(format!("ask{}_qty", i + 1).as_str(), qty)?;
    }

    row.at(TimestampNanos::now())?;
    Ok(())
}

fn append_book_ticker(buf: &mut Buffer, d: &BookTickerData) -> questdb::Result<()> {
    buf.table("book_ticker")?
        .symbol("symbol", &d.symbol)?
        .column_f64("bid_price", d.bid_price.to_f64().unwrap_or(f64::NAN))?
        .column_f64("bid_qty", d.bid_quantity.parse::<f64>().unwrap_or(0.0))?
        .column_f64("ask_price", d.ask_price.to_f64().unwrap_or(f64::NAN))?
        .column_f64("ask_qty", d.ask_quantity.parse::<f64>().unwrap_or(0.0))?
        .at(TimestampNanos::now())?;
    Ok(())
}

fn append_fng(buf: &mut Buffer, d: &FngData) -> questdb::Result<()> {
    let ts_seconds = d.timestamp.parse::<i64>().unwrap_or(0);

    // 타임스탬프가 0이면 잘못된 데이터이므로 저장하지 않음
    if ts_seconds == 0 {
        return Ok(());
    }

    buf.table("fear_greed_index")?
        .symbol("status", d.status.as_str())?
        .column_i64("value", d.value.parse::<i64>().unwrap_or(0))?
        .at(TimestampNanos::new(ts_seconds * 1_000_000_000))?;

    Ok(())
}
