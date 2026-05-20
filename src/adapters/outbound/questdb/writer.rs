use crate::config::QuestDbRuntimeConfig;
use crate::domain::event::MarketDataEvent;
use crate::domain::market_data::{BookTicker, Depth, FearGreed, Kline, MarkPrice, Trade};
use crate::domain::order::Fill;
use crate::ports::market_data_sink::MarketDataSink;
use async_trait::async_trait;
use questdb::ingress::{Buffer, ProtocolVersion, Sender, TimestampNanos};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

const BID_PRICE_COLS: [&str; 10] = [
    "bid1_price",
    "bid2_price",
    "bid3_price",
    "bid4_price",
    "bid5_price",
    "bid6_price",
    "bid7_price",
    "bid8_price",
    "bid9_price",
    "bid10_price",
];
const BID_QTY_COLS: [&str; 10] = [
    "bid1_qty",
    "bid2_qty",
    "bid3_qty",
    "bid4_qty",
    "bid5_qty",
    "bid6_qty",
    "bid7_qty",
    "bid8_qty",
    "bid9_qty",
    "bid10_qty",
];
const ASK_PRICE_COLS: [&str; 10] = [
    "ask1_price",
    "ask2_price",
    "ask3_price",
    "ask4_price",
    "ask5_price",
    "ask6_price",
    "ask7_price",
    "ask8_price",
    "ask9_price",
    "ask10_price",
];
const ASK_QTY_COLS: [&str; 10] = [
    "ask1_qty",
    "ask2_qty",
    "ask3_qty",
    "ask4_qty",
    "ask5_qty",
    "ask6_qty",
    "ask7_qty",
    "ask8_qty",
    "ask9_qty",
    "ask10_qty",
];

pub struct QuestDbSink {
    url: String,
    cfg: QuestDbRuntimeConfig,
}

impl QuestDbSink {
    pub fn new(url: String, cfg: QuestDbRuntimeConfig) -> Self {
        Self { url, cfg }
    }
}

#[async_trait]
impl MarketDataSink for QuestDbSink {
    async fn run(self: Box<Self>, rx: Receiver<MarketDataEvent>, token: CancellationToken) {
        run(&self.url, self.cfg, rx, token).await;
    }
}

pub async fn run(
    url: &str,
    cfg: QuestDbRuntimeConfig,
    mut market_rx: Receiver<MarketDataEvent>,
    token: CancellationToken,
) {
    let mut sender = match Sender::from_conf(url) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "QuestDB 초기 연결 실패");
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
                    MarketDataEvent::FearGreed(d) => append_fng(&mut buf, d),
                };

                match result {
                    Ok(()) => rows += 1,
                    Err(e) => {
                        warn!(error = %e, "Market data append 실패 — 이전 누적 buffer flush 시도");
                        flush_if_any(&mut sender, &mut buf, &mut rows, url, "append_error");
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
            error!(reason = %reason, rows = %rows, error = %e, "QuestDB flush 실패 — 재연결 시도");
            buf.clear();
            *rows = 0;
            match Sender::from_conf(url) {
                Ok(new) => {
                    *sender = new;
                    info!("QuestDB 재연결 성공");
                }
                Err(e2) => error!(error = %e2, "QuestDB 재연결 실패"),
            }
        }
    }
}

fn append_trade(buf: &mut Buffer, d: &Trade) -> questdb::Result<()> {
    buf.table("trades")?
        .symbol("symbol", &d.symbol)?
        .column_dec("price", &d.price)?
        .column_dec("quantity", &d.quantity)?
        .column_bool("buyer_is_market_maker", d.buyer_is_market_maker)?
        .at(TimestampNanos::new(d.time_ns))?;

    Ok(())
}

fn append_depth(buf: &mut Buffer, d: &Depth) -> questdb::Result<()> {
    for i in 0..10 {
        if d.bids.get(i).is_none() {
            warn!(symbol = %d.symbol, side = "bid", level = i + 1, total = d.bids.len(), "depth level 부족");
            return Ok(());
        }

        if d.asks.get(i).is_none() {
            warn!(symbol = %d.symbol, side = "ask", level = i + 1, total = d.asks.len(), "depth level 부족");
            return Ok(());
        }
    }

    let row = buf.table("depth")?;
    row.symbol("symbol", &d.symbol)?
        .column_i64("first_update_id", d.first_update_id)?
        .column_i64("last_update_id", d.last_update_id)?;

    for i in 0..10 {
        row.column_dec(BID_PRICE_COLS[i], &d.bids[i].price)?
            .column_dec(BID_QTY_COLS[i], &d.bids[i].quantity)?
            .column_dec(ASK_PRICE_COLS[i], &d.asks[i].price)?
            .column_dec(ASK_QTY_COLS[i], &d.asks[i].quantity)?;
    }

    row.at(TimestampNanos::new(d.event_time_ns))?;
    Ok(())
}

fn append_book_ticker(buf: &mut Buffer, d: &BookTicker) -> questdb::Result<()> {
    buf.table("book_ticker")?
        .symbol("symbol", &d.symbol)?
        .column_dec("bid_price", &d.bid_price)?
        .column_dec("bid_qty", &d.bid_quantity)?
        .column_dec("ask_price", &d.ask_price)?
        .column_dec("ask_qty", &d.ask_quantity)?
        .at(TimestampNanos::new(d.event_time_ns))?;
    Ok(())
}

fn append_kline(buf: &mut Buffer, d: &Kline) -> questdb::Result<()> {
    buf.table("kline")?
        .symbol("symbol", &d.symbol)?
        .column_i64("open_time", d.open_time_ms)?
        .column_dec("open", &d.open)?
        .column_dec("high", &d.high)?
        .column_dec("low", &d.low)?
        .column_dec("close", &d.close)?
        .column_dec("volume", &d.volume)?
        .column_dec("quote_volume", &d.quote_volume)?
        .column_i64("num_trades", d.num_trades)?
        .column_bool("is_closed", d.is_closed)?
        .at(TimestampNanos::new(d.event_time_ns))?;
    Ok(())
}

fn append_mark_price(buf: &mut Buffer, d: &MarkPrice) -> questdb::Result<()> {
    buf.table("mark_price")?
        .symbol("symbol", &d.symbol)?
        .column_dec("mark_price", &d.mark_price)?
        .column_dec("index_price", &d.index_price)?
        .column_dec("funding_rate", &d.funding_rate)?
        .column_i64("next_funding_time", d.next_funding_time_ms)?
        .at(TimestampNanos::new(d.event_time_ns))?;
    Ok(())
}

fn append_fng(buf: &mut Buffer, d: &FearGreed) -> questdb::Result<()> {
    // 타임스탬프가 0이면 잘못된 데이터이므로 저장하지 않음
    if d.timestamp_sec == 0 {
        return Ok(());
    }

    buf.table("fear_greed_index")?
        .symbol("status", d.status.as_str())?
        .column_str("value", &d.value)?
        .at(TimestampNanos::new(d.timestamp_sec * 1_000_000_000))?;

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
