use crate::adapters::outbound::questdb::aggregator::MinuteAggregator;
use crate::config::QuestDbRuntimeConfig;
use crate::domain::event::MarketDataEvent;
use crate::domain::market_data::{FearGreed, Kline, Trade};
use crate::ports::market_data_sink::MarketDataSink;
use async_trait::async_trait;
use questdb::ingress::{Buffer, ProtocolVersion, Sender, TimestampNanos};
use std::time::Duration;
use tokio::sync::mpsc::Receiver;
use tokio::time::{MissedTickBehavior, interval};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

/// QuestDB line protocol writer sink입니다.
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
    let mut agg = MinuteAggregator::new();

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

                // 분 경계 확인 → 이전 분 feature 행 flush
                let ts_ns = event_time_ns(&event);
                if ts_ns > 0 {
                    match agg.maybe_flush_minute(ts_ns, &mut buf) {
                        Ok(n) => rows += n,
                        Err(e) => {
                            warn!(error = %e, "aggregator flush 실패 — buffer 초기화");
                            buf.clear();
                            rows = 0;
                        }
                    }
                }

                // aggregator 상태 업데이트
                match &event {
                    MarketDataEvent::Trade(d)      => agg.update_trade(d),
                    MarketDataEvent::BookTicker(d) => agg.update_book_ticker(d),
                    MarketDataEvent::Depth(d)      => agg.update_depth(d),
                    MarketDataEvent::MarkPrice(d)  => agg.update_mark_price(d),
                    _ => {}
                }

                // raw 저장: trade(TTL 3일), kline, fear_greed_index만 유지
                let prev_len = buf.len();
                let result = match &event {
                    MarketDataEvent::Trade(d)     => append_raw_trade(&mut buf, d),
                    MarketDataEvent::Kline(d)     => append_kline(&mut buf, d),
                    MarketDataEvent::FearGreed(d) => append_fng(&mut buf, d),
                    _ => Ok(()),
                };

                match result {
                    Ok(()) => {
                        if buf.len() > prev_len {
                            rows += 1;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "Market data append 실패 — buffer 초기화");
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

fn event_time_ns(event: &MarketDataEvent) -> i64 {
    match event {
        MarketDataEvent::Trade(d) => d.time_ns,
        MarketDataEvent::Depth(d) => d.event_time_ns,
        MarketDataEvent::BookTicker(d) => d.event_time_ns,
        MarketDataEvent::Kline(d) => d.event_time_ns,
        MarketDataEvent::MarkPrice(d) => d.event_time_ns,
        MarketDataEvent::FearGreed(d) => d.timestamp_sec * 1_000_000_000,
    }
}

fn append_raw_trade(buf: &mut Buffer, d: &Trade) -> questdb::Result<()> {
    buf.table("raw_trade")?
        .symbol("symbol", &d.symbol)?
        .column_dec("price", &d.price)?
        .column_dec("quantity", &d.quantity)?
        .column_bool("buyer_is_market_maker", d.buyer_is_market_maker)?
        .at(TimestampNanos::new(d.time_ns))?;
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

fn append_fng(buf: &mut Buffer, d: &FearGreed) -> questdb::Result<()> {
    if d.timestamp_sec == 0 {
        return Ok(());
    }
    buf.table("fear_greed_index")?
        .symbol("status", d.status.as_str())?
        .column_str("value", &d.value)?
        .at(TimestampNanos::new(d.timestamp_sec * 1_000_000_000))?;
    Ok(())
}
