use crate::market_data::alternative::dto::FngData;
use crate::market_data::binance::dto::{BookTickerData, DepthData, TradeData};
use crate::storage::event::StorageEvent;
use questdb::ingress::{Buffer, ProtocolVersion, Sender, TimestampNanos};
use rust_decimal::prelude::ToPrimitive;
use std::f64;
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const BATCH_MAX_ROWS: usize = 5_000;
const BATCH_INTERVAL: Duration = Duration::from_millis(100);
const BUFFER_MAX_BYTES: usize = 1024 * 1024;

pub async fn run(url: &str, mut rx: Receiver<StorageEvent>, token: CancellationToken) {
    let mut sender = match Sender::from_conf(url) {
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
                    flush_if_any(&mut sender, &mut buf, &mut rows, url, "shutdown");
                    info!("DB writer 종료");
                    return;
                };

                let result = match &event {
                    StorageEvent::Trade(d) => append_trade(&mut buf, d),
                    StorageEvent::Depth(d) => append_depth(&mut buf, d),
                    StorageEvent::BookTicker(d) => append_book_ticker(&mut buf, d),
                    StorageEvent::Fng(d) => append_fng(&mut buf, d),
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
                    flush_if_any(&mut sender, &mut buf, &mut rows, url, "size");
                }
            }
            _ = ticker.tick() => {
                flush_if_any(&mut sender, &mut buf, &mut rows, url, "tick");
            }
            _ = token.cancelled() => {
                flush_if_any(&mut sender, &mut buf, &mut rows, url, "shutdown");
                info!("QuestDB writer 종료");
                return;
            }
        }
    }
}

fn flush_if_any(sender: &mut Sender, buf: &mut Buffer, rows: &mut usize, url: &str, reason: &str) {
    if *rows == 0 {
        return;
    }

    match sender.flush(buf) {
        Ok(()) => *rows = 0,
        Err(e) => {
            error!("QuestDB flush 실패 (reason={reason}, rows={rows}): {e}, 재연결 시도");
            buf.clear();
            *rows = 0;
            match Sender::from_conf(url) {
                Ok(new) => {
                    *sender = new;
                    info!("QuestDB 재연결 성공");
                }
                Err(e2) => error!("QuestDB 재연결 실패: {e2}"),
            }
        }
    }
}

fn append_trade(buf: &mut Buffer, d: &TradeData) -> questdb::Result<()> {
    let (Some(price), Some(qty)) = (d.price.to_f64(), d.quantity.to_f64()) else {
        warn!(
            "trade decimal -> f64 실패: symbol={}, price={}, qty={}",
            d.symbol, d.price, d.quantity
        );
        return Ok(());
    };

    buf.table("trades")?
        .symbol("symbol", &d.symbol)?
        .column_f64("price", price)?
        .column_f64("quantity", qty)?
        .column_bool("buyer_is_market_maker", d.buyer_is_market_maker)?
        .at(TimestampNanos::new(d.time as i64 * 1_000_000))?;

    Ok(())
}

fn append_depth(buf: &mut Buffer, d: &DepthData) -> questdb::Result<()> {
    let mut bids = [(0.0_f64, 0.0_f64); 10];
    let mut asks = [(0.0_f64, 0.0_f64); 10];

    for i in 0..10 {
        let Some(bid) = d.bids.get(i) else {
            warn!(
                "depth level 부족: symbol={}, side=bid, level={}, len={}",
                d.symbol,
                i + 1,
                d.bids.len()
            );
            return Ok(());
        };
        let (Some(bid_price), Some(bid_qty)) = (bid.0.to_f64(), bid.1.to_f64()) else {
            warn!(
                "depth decimal -> f64 실패: symbol={}, side=bid, level={}",
                d.symbol,
                i + 1
            );
            return Ok(());
        };
        bids[i] = (bid_price, bid_qty);

        let Some(ask) = d.asks.get(i) else {
            warn!(
                "depth level 부족: symbol={}, side=ask, level={}, len={}",
                d.symbol,
                i + 1,
                d.asks.len()
            );
            return Ok(());
        };
        let (Some(ask_price), Some(ask_qty)) = (ask.0.to_f64(), ask.1.to_f64()) else {
            warn!(
                "depth decimal -> f64 실패: symbol={}, side=ask, level={}",
                d.symbol,
                i + 1
            );
            return Ok(());
        };
        asks[i] = (ask_price, ask_qty);
    }

    let row = buf.table("depth")?;
    row.symbol("symbol", &d.symbol)?
        .column_i64("first_update_id", d.first_update_id as i64)?
        .column_i64("last_update_id", d.last_update_id as i64)?;

    for i in 0..10 {
        let (bid_price, bid_qty) = bids[i];
        let (ask_price, ask_qty) = asks[i];

        row.column_f64(format!("bid{}_price", i + 1).as_str(), bid_price)?
            .column_f64(format!("bid{}_qty", i + 1).as_str(), bid_qty)?
            .column_f64(format!("ask{}_price", i + 1).as_str(), ask_price)?
            .column_f64(format!("ask{}_qty", i + 1).as_str(), ask_qty)?;
    }

    row.at(TimestampNanos::new(d.event_time as i64 * 1_000_000))?;
    Ok(())
}

fn append_book_ticker(buf: &mut Buffer, d: &BookTickerData) -> questdb::Result<()> {
    let (Some(bid_price), Some(ask_price)) = (d.bid_price.to_f64(), d.ask_price.to_f64()) else {
        warn!(
            "book ticker decimal -> f64 실패: symbol={}, bid_price={}, ask_price={}",
            d.symbol, d.bid_price, d.ask_price
        );
        return Ok(());
    };

    let (Ok(bid_qty), Ok(ask_qty)) = (d.bid_quantity.parse::<f64>(), d.ask_quantity.parse::<f64>())
    else {
        warn!(
            "book ticker string -> f64 실패: symbol={}, bid_price={}, ask_price={}",
            d.symbol, d.bid_quantity, d.ask_quantity
        );
        return Ok(());
    };

    buf.table("book_ticker")?
        .symbol("symbol", &d.symbol)?
        .column_f64("bid_price", bid_price)?
        .column_f64("bid_qty", bid_qty)?
        .column_f64("ask_price", ask_price)?
        .column_f64("ask_qty", ask_qty)?
        .at(TimestampNanos::new(d.event_time as i64 * 1_000_000))?;
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
