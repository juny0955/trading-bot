use questdb::ingress::{Buffer, ProtocolVersion, Sender, TimestampNanos};
use rust_decimal::prelude::ToPrimitive;
use std::f64;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time::{MissedTickBehavior, interval};
use tracing::{error, info, warn};

use crate::dtos::{BookTickerData, DepthData, FngData, TradeData};

pub enum DbEvent {
    Trade(TradeData),
    Depth(DepthData),
    BookTicker(BookTickerData),
    Fng(FngData),
}

const BATCH_MAX_ROWS: usize = 5_000;
const BATCH_INTERVAL: Duration = Duration::from_millis(100);
const BUFFER_MAX_BYTES: usize = 1024 * 1024;

pub async fn run(conf: &str, mut rx: Receiver<DbEvent>) {
    let mut sender = match Sender::from_conf(conf) {
        Ok(s) => s,
        Err(e) => {
            error!("QuestDB 초기 연결 실패: {e}");
            return;
        }
    };

    let mut buf = Buffer::new(ProtocolVersion::V2);
    let mut rows: usize = 0;

    let mut ticker = interval(BATCH_INTERVAL);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe = rx.recv() => {
                let Some(event) = maybe else {
                    flush_if_any(&mut sender, &mut buf, &mut rows, conf, "shutdown");
                    info!("DB writer 종료");
                    return;
                };

                let result = match &event {
                     DbEvent::Trade(d) => append_trade(&mut buf, d),
                    DbEvent::Depth(d) => append_depth(&mut buf, d),
                    DbEvent::BookTicker(d) => append_book_ticker(&mut buf, d),
                    DbEvent::Fng(d) => append_fng(&mut buf, d),
                };

                match result {
                    Ok(()) => rows += 1,
                    Err(e) => {
                        warn!("append 실패 누적 buffer 폐기: {e}");
                        buf.clear();
                        rows = 0;
                    }
                }

                if rows <= BATCH_MAX_ROWS || buf.len() >= BUFFER_MAX_BYTES {
                    flush_if_any(&mut sender, &mut buf, &mut rows, conf, "size");
                }
            }
            _ = ticker.tick() => {
                flush_if_any(&mut sender, &mut buf, &mut rows, conf, "tick");
            }
        }
    }
}

fn flush_if_any(sender: &mut Sender, buf: &mut Buffer, rows: &mut usize, conf: &str, reason: &str) {
    if *rows == 0 {
        return;
    }

    match sender.flush(buf) {
        Ok(()) => *rows = 0,
        Err(e) => {
            error!("QuestDB flush 실패 (reason={reason}, rows={rows}): {e}, 재연결 시도");
            buf.clear();
            *rows = 0;
            match Sender::from_conf(conf) {
                Ok(new) => {
                    *sender = new;
                    info!("QeustDB 재연결 성공");
                }
                Err(e2) => error!("QuestDB 재연결 실패: {e2}"),
            }
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
