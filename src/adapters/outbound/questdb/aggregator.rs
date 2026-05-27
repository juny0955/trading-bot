use crate::domain::market_data::{BookTicker, Depth, MarkPrice, Trade};
use questdb::ingress::{Buffer, TimestampNanos};
use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;
use std::collections::HashMap;

const MINUTE_NS: i64 = 60_000_000_000;

pub fn minute_bucket(event_time_ns: i64) -> i64 {
    event_time_ns - (event_time_ns % MINUTE_NS)
}

fn d(v: &Decimal) -> f64 {
    v.to_f64().unwrap_or(0.0)
}

// ── Trade accumulator (per symbol) ──────────────────────────────────
// OHLCV는 kline 테이블 참조. buy/sell split + vwap + delta만 저장.

#[derive(Default)]
struct TradeAcc {
    buy_volume: Decimal,
    sell_volume: Decimal,
    trade_count: i64,
    vwap_num: Decimal,
    total_volume: Decimal,
}

impl TradeAcc {
    fn update(&mut self, t: &Trade) {
        // buyer_is_market_maker=true → taker가 매도, false → taker가 매수
        if t.buyer_is_market_maker {
            self.sell_volume += t.quantity;
        } else {
            self.buy_volume += t.quantity;
        }
        self.total_volume += t.quantity;
        self.trade_count += 1;
        self.vwap_num += t.price * t.quantity;
    }

    fn flush(&self, buf: &mut Buffer, symbol: &str, ts_ns: i64) -> questdb::Result<()> {
        if self.trade_count == 0 {
            return Ok(());
        }
        let vwap = if self.total_volume.is_zero() {
            0.0
        } else {
            d(&self.vwap_num) / d(&self.total_volume)
        };
        let delta_volume = d(&self.buy_volume) - d(&self.sell_volume);

        buf.table("trade_features_1m")?
            .symbol("symbol", symbol)?
            .column_f64("buy_volume", d(&self.buy_volume))?
            .column_f64("sell_volume", d(&self.sell_volume))?
            .column_f64("delta_volume", delta_volume)?
            .column_i64("trade_count", self.trade_count)?
            .column_f64("vwap", vwap)?
            .at(TimestampNanos::new(ts_ns))?;
        Ok(())
    }
}

// ── Orderbook accumulator (per symbol) ──────────────────────────────
// 분 끝 스냅샷(레벨 prices) + spread/imbalance 통계 집계

#[derive(Default)]
struct OrderbookAcc {
    bid: [Option<(Decimal, Decimal)>; 3],
    ask: [Option<(Decimal, Decimal)>; 3],
    spread_sum: f64,
    spread_max: f64,
    imbalance_sum: f64,
    imbalance_sum_sq: f64,
    sample_count: i64,
}

impl OrderbookAcc {
    fn update_book_ticker(&mut self, bt: &BookTicker) {
        self.bid[0] = Some((bt.bid_price, bt.bid_quantity));
        self.ask[0] = Some((bt.ask_price, bt.ask_quantity));
        self.accumulate_stats();
    }

    fn update_depth(&mut self, d: &Depth) {
        for i in 0..3 {
            self.bid[i] = d.bids.get(i).map(|l| (l.price, l.quantity));
            self.ask[i] = d.asks.get(i).map(|l| (l.price, l.quantity));
        }
        self.accumulate_stats();
    }

    fn accumulate_stats(&mut self) {
        let (Some((ask1_p, _)), Some((bid1_p, _))) = (self.ask[0], self.bid[0]) else {
            return;
        };
        let spread = d(&ask1_p) - d(&bid1_p);
        if spread < 0.0 {
            return;
        }

        let bid_total: f64 = self.bid.iter()
            .filter_map(|s| s.map(|(_, q)| d(&q)))
            .sum();
        let ask_total: f64 = self.ask.iter()
            .filter_map(|s| s.map(|(_, q)| d(&q)))
            .sum();
        let imbalance = if bid_total + ask_total > 0.0 {
            bid_total / (bid_total + ask_total)
        } else {
            0.5
        };

        self.spread_sum += spread;
        if spread > self.spread_max {
            self.spread_max = spread;
        }
        self.imbalance_sum += imbalance;
        self.imbalance_sum_sq += imbalance * imbalance;
        self.sample_count += 1;
    }

    fn flush(&self, buf: &mut Buffer, symbol: &str, ts_ns: i64) -> questdb::Result<()> {
        if self.sample_count == 0 {
            return Ok(());
        }
        let n = self.sample_count as f64;
        let spread_avg = self.spread_sum / n;
        let imbalance_avg = self.imbalance_sum / n;
        let imbalance_var = (self.imbalance_sum_sq / n) - (imbalance_avg * imbalance_avg);
        let imbalance_std = imbalance_var.max(0.0).sqrt();

        const BID_P: [&str; 3] = ["bid1_price", "bid2_price", "bid3_price"];
        const BID_Q: [&str; 3] = ["bid1_qty",   "bid2_qty",   "bid3_qty"  ];
        const ASK_P: [&str; 3] = ["ask1_price", "ask2_price", "ask3_price"];
        const ASK_Q: [&str; 3] = ["ask1_qty",   "ask2_qty",   "ask3_qty"  ];

        let row = buf.table("orderbook_features_1m")?;
        row.symbol("symbol", symbol)?;

        for i in 0..3 {
            if let Some((p, q)) = self.bid[i] {
                row.column_f64(BID_P[i], d(&p))?.column_f64(BID_Q[i], d(&q))?;
            }
            if let Some((p, q)) = self.ask[i] {
                row.column_f64(ASK_P[i], d(&p))?.column_f64(ASK_Q[i], d(&q))?;
            }
        }

        row.column_f64("spread_avg",    spread_avg)?
            .column_f64("spread_max",    self.spread_max)?
            .column_f64("imbalance_avg", imbalance_avg)?
            .column_f64("imbalance_std", imbalance_std)?
            .at(TimestampNanos::new(ts_ns))?;
        Ok(())
    }
}

// ── Mark price state (per symbol, last value) ─────────────────────────

#[derive(Clone)]
struct MarkPriceState {
    mark_price: Decimal,
    index_price: Decimal,
    funding_rate: Decimal,
    next_funding_time_ms: i64,
}

impl From<&MarkPrice> for MarkPriceState {
    fn from(m: &MarkPrice) -> Self {
        Self {
            mark_price: m.mark_price,
            index_price: m.index_price,
            funding_rate: m.funding_rate,
            next_funding_time_ms: m.next_funding_time_ms,
        }
    }
}

impl MarkPriceState {
    fn flush(&self, buf: &mut Buffer, symbol: &str, ts_ns: i64) -> questdb::Result<()> {
        buf.table("mark_price_1m")?
            .symbol("symbol", symbol)?
            .column_f64("mark_price",   d(&self.mark_price))?
            .column_f64("index_price",  d(&self.index_price))?
            .column_f64("funding_rate", d(&self.funding_rate))?
            .column_i64("next_funding_time", self.next_funding_time_ms)?
            .at(TimestampNanos::new(ts_ns))?;
        Ok(())
    }
}

// ── Public MinuteAggregator ───────────────────────────────────────────

#[derive(Default)]
pub struct MinuteAggregator {
    current_minute: i64,
    trades: HashMap<String, TradeAcc>,
    orderbooks: HashMap<String, OrderbookAcc>,
    mark_prices: HashMap<String, MarkPriceState>,
}

impl MinuteAggregator {
    pub fn new() -> Self {
        Self::default()
    }

    /// 분 경계를 넘었으면 이전 분의 feature row를 buf에 기록하고 집계를 초기화한다.
    /// 기록된 행 수를 반환한다.
    pub fn maybe_flush_minute(
        &mut self,
        event_time_ns: i64,
        buf: &mut Buffer,
    ) -> questdb::Result<usize> {
        let bucket = minute_bucket(event_time_ns);
        if self.current_minute == 0 {
            self.current_minute = bucket;
            return Ok(0);
        }
        if bucket <= self.current_minute {
            return Ok(0);
        }

        let flush_ts = self.current_minute + MINUTE_NS - 1;
        let mut count = 0;

        let symbols: Vec<String> = self
            .trades
            .keys()
            .chain(self.orderbooks.keys())
            .chain(self.mark_prices.keys())
            .cloned()
            .collect::<std::collections::HashSet<_>>()
            .into_iter()
            .collect();

        for sym in &symbols {
            if let Some(acc) = self.trades.get(sym) {
                let prev = buf.len();
                acc.flush(buf, sym, flush_ts)?;
                if buf.len() > prev {
                    count += 1;
                }
            }
            if let Some(ob) = self.orderbooks.get(sym) {
                let prev = buf.len();
                ob.flush(buf, sym, flush_ts)?;
                if buf.len() > prev {
                    count += 1;
                }
            }
            if let Some(mp) = self.mark_prices.get(sym) {
                let prev = buf.len();
                mp.flush(buf, sym, flush_ts)?;
                if buf.len() > prev {
                    count += 1;
                }
            }
        }

        self.trades.clear();
        self.orderbooks.clear();
        // mark_prices는 유지 (다음 분에도 최신 값 사용)
        self.current_minute = bucket;
        Ok(count)
    }

    pub fn update_trade(&mut self, t: &Trade) {
        self.trades.entry(t.symbol.clone()).or_default().update(t);
    }

    pub fn update_book_ticker(&mut self, bt: &BookTicker) {
        self.orderbooks
            .entry(bt.symbol.clone())
            .or_default()
            .update_book_ticker(bt);
    }

    pub fn update_depth(&mut self, d: &Depth) {
        self.orderbooks
            .entry(d.symbol.clone())
            .or_default()
            .update_depth(d);
    }

    pub fn update_mark_price(&mut self, m: &MarkPrice) {
        self.mark_prices
            .insert(m.symbol.clone(), MarkPriceState::from(m));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_trade(symbol: &str, price: Decimal, qty: Decimal, maker: bool, time_ns: i64) -> Trade {
        Trade {
            symbol: symbol.to_string(),
            price,
            quantity: qty,
            buyer_is_market_maker: maker,
            time_ns,
        }
    }

    #[test]
    fn trade_acc_buy_sell_split() {
        let mut acc = TradeAcc::default();
        acc.update(&make_trade("BTCUSDT", dec!(100), dec!(2), false, 0)); // taker buy
        acc.update(&make_trade("BTCUSDT", dec!(100), dec!(1), true, 0));  // taker sell
        assert_eq!(acc.buy_volume, dec!(2));
        assert_eq!(acc.sell_volume, dec!(1));
        assert_eq!(acc.trade_count, 2);
        let delta = acc.buy_volume - acc.sell_volume;
        assert_eq!(delta, dec!(1));
    }

    #[test]
    fn trade_acc_vwap() {
        let mut acc = TradeAcc::default();
        acc.update(&make_trade("BTCUSDT", dec!(100), dec!(1), false, 0));
        acc.update(&make_trade("BTCUSDT", dec!(200), dec!(1), false, 0));
        // vwap = (100*1 + 200*1) / 2 = 150
        let vwap = acc.vwap_num / acc.total_volume;
        assert_eq!(vwap, dec!(150));
    }

    #[test]
    fn minute_bucket_aligns_to_minute() {
        let ts = 90_500_000_000i64; // 1분 30.5초
        let bucket = minute_bucket(ts);
        assert_eq!(bucket, 60_000_000_000); // 1분 버킷
    }

    #[test]
    fn aggregator_no_flush_within_same_minute() {
        let mut agg = MinuteAggregator::new();
        let mut buf = questdb::ingress::Buffer::new(questdb::ingress::ProtocolVersion::V2);
        let t1 = 60_000_000_000i64; // 분 시작
        let t2 = 90_000_000_000i64; // 같은 분 내
        agg.maybe_flush_minute(t1, &mut buf).unwrap();
        let n = agg.maybe_flush_minute(t2, &mut buf).unwrap();
        assert_eq!(n, 0);
    }
}
