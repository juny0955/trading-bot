use crate::binance::dto::{BookTickerData, DepthData, KlineData, MarkPriceData, TradeData};
use crate::config::QuestDbRuntimeConfig;
use crate::market_data::alternative::dto::FngData;
use crate::order::types::Fill;
use crate::storage::event::MarketDataEvent;
use questdb::ingress::{Buffer, ProtocolVersion, Sender, TimestampNanos};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

pub async fn run(
    url: &str,
    cfg: QuestDbRuntimeConfig,
    mut market_rx: Receiver<MarketDataEvent>,
    token: CancellationToken,
) {
    let mut sender = match Sender::from_conf(url) {
        Ok(s) => s,
        Err(e) => {
            error!("QuestDB 초기 연결 실패: {e}");
            return;
        }
    };

    let mut buf = Buffer::new(ProtocolVersion::V3);
    let mut rows: usize = 0;

    let mut ticker = interval(Duration::from_millis(cfg.batch_interval_ms));
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe = market_rx.recv() => {
                let Some(event) = maybe else {
                    flush_if_any(&mut sender, &mut buf, &mut rows, url, "shutdown");
                    info!("DB writer 종료");
                    return;
                };

                let result = match &event {
                    MarketDataEvent::Trade(d) => append_trade(&mut buf, d),
                    MarketDataEvent::Depth(d) => append_depth(&mut buf, d),
                    MarketDataEvent::BookTicker(d) => append_book_ticker(&mut buf, d),
                    MarketDataEvent::Kline(d) => append_kline(&mut buf, d),
                    MarketDataEvent::MarkPrice(d) => append_mark_price(&mut buf, d),
                    MarketDataEvent::Fng(d) => append_fng(&mut buf, d),
                };

                match result {
                    Ok(()) => rows += 1,
                    Err(e) => {
                        warn!("Market data append 실패 누적 buffer 폐기: {e}");
                        buf.clear();
                        rows = 0;
                    }
                }

                if rows >= cfg.batch_max_rows || buf.len() >= cfg.buffer_max_bytes {
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
    buf.table("trades")?
        .symbol("symbol", &d.symbol)?
        .column_dec("price", &d.price)?
        .column_dec("quantity", &d.quantity)?
        .column_bool("buyer_is_market_maker", d.buyer_is_market_maker)?
        .at(TimestampNanos::new(d.time * 1_000_000))?;

    Ok(())
}

fn append_depth(buf: &mut Buffer, d: &DepthData) -> questdb::Result<()> {
    for i in 0..10 {
        if d.bids.get(i).is_none() {
            warn!(
                "depth level 부족: symbol={}, side=bid, level={}, len={}",
                d.symbol,
                i + 1,
                d.bids.len()
            );
            return Ok(());
        }

        if d.asks.get(i).is_none() {
            warn!(
                "depth level 부족: symbol={}, side=ask, level={}, len={}",
                d.symbol,
                i + 1,
                d.asks.len()
            );
            return Ok(());
        }
    }

    let row = buf.table("depth")?;
    row.symbol("symbol", &d.symbol)?
        .column_i64("first_update_id", d.first_update_id)?
        .column_i64("last_update_id", d.last_update_id)?;

    for i in 0..10 {
        row.column_dec(format!("bid{}_price", i + 1).as_str(), &d.bids[i].0)?
            .column_dec(format!("bid{}_qty", i + 1).as_str(), &d.bids[i].1)?
            .column_dec(format!("ask{}_price", i + 1).as_str(), &d.asks[i].0)?
            .column_dec(format!("ask{}_qty", i + 1).as_str(), &d.asks[i].1)?;
    }

    row.at(TimestampNanos::new(d.event_time * 1_000_000))?;
    Ok(())
}

fn append_book_ticker(buf: &mut Buffer, d: &BookTickerData) -> questdb::Result<()> {
    buf.table("book_ticker")?
        .symbol("symbol", &d.symbol)?
        .column_dec("bid_price", &d.bid_price)?
        .column_dec("bid_qty", &d.bid_quantity)?
        .column_dec("ask_price", &d.ask_price)?
        .column_dec("ask_qty", &d.ask_quantity)?
        .at(TimestampNanos::new(d.event_time * 1_000_000))?;
    Ok(())
}

fn append_kline(buf: &mut Buffer, d: &KlineData) -> questdb::Result<()> {
    buf.table("kline")?
        .symbol("symbol", &d.symbol)?
        .column_i64("open_time", d.kline.open_time)?
        .column_dec("open", &d.kline.open)?
        .column_dec("high", &d.kline.high)?
        .column_dec("low", &d.kline.low)?
        .column_dec("close", &d.kline.close)?
        .column_dec("volume", &d.kline.volume)?
        .column_dec("quote_volume", &d.kline.quote_volume)?
        .column_i64("num_trades", d.kline.num_trades)?
        .column_bool("is_closed", d.kline.is_closed)?
        .at(TimestampNanos::new(d.event_time * 1_000_000))?;
    Ok(())
}

fn append_mark_price(buf: &mut Buffer, d: &MarkPriceData) -> questdb::Result<()> {
    buf.table("mark_price")?
        .symbol("symbol", &d.symbol)?
        .column_dec("mark_price", &d.mark_price)?
        .column_dec("index_price", &d.index_price)?
        .column_dec("funding_rate", &d.funding_rate)?
        .column_i64("next_funding_time", d.next_funding_time)?
        .at(TimestampNanos::new(d.event_time * 1_000_000))?;
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
        .column_str("value", &d.value)?
        .at(TimestampNanos::new(ts_seconds * 1_000_000_000))?;

    Ok(())
}

fn _append_fill(buf: &mut Buffer, d: &Fill) -> questdb::Result<()> {
    buf.table("fill")?
        .symbol("symbol", &d.symbol)?
        .symbol("side", &format!("{:?}", d.side).to_lowercase())?
        .symbol("fee_asset", &d.fee_asset)?
        .column_str("order_id", &d.order_id.to_string())?
        .column_dec("qty", &d.qty)?
        .column_dec("price", &d.price)?
        .column_dec("fee", &d.fee)?
        .at(TimestampNanos::new(
            d.filled_at.timestamp_nanos_opt().unwrap_or(0),
        ))?;
    Ok(())
}
