use crate::backtest::data::{BarQuery, DepthQuery, MarketDataSource};
use crate::backtest::portfolio::Portfolio;
use crate::backtest::strategy::{Context, Strategy};
use crate::backtest::types::{BacktestOrder, Bar, DepthSnapshot, Event, Side};
use anyhow::Result;
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;
use tracing::info;

pub struct BacktestConfig {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub bar_interval: String,
    pub depth_every: String,
    pub initial_equity: Decimal,
    pub fee_bps: Decimal,
    pub depth_freshness_sec: i64,
}

#[derive(Debug, Clone)]
pub struct EquityPoint {
    pub ts_ns: i64,
    pub equity: Decimal,
    pub position_qty: Decimal,
    pub mark_price: Decimal,
}

pub struct BacktestResult {
    pub portfolio: Portfolio,
    pub equity_curve: Vec<EquityPoint>,
}

pub async fn run(
    src: &dyn MarketDataSource,
    cfg: BacktestConfig,
    strategy: &mut dyn Strategy,
) -> Result<BacktestResult> {
    let (bars, depths) = tokio::join!(
        src.fetch_bars(BarQuery {
            symbol: cfg.symbol.clone(),
            from_ns: cfg.from_ns,
            to_ns: cfg.to_ns,
            interval: cfg.bar_interval.clone(),
        }),
        src.fetch_depth_snapshots(DepthQuery {
            symbol: cfg.symbol.clone(),
            from_ns: cfg.from_ns,
            to_ns: cfg.to_ns,
            every: cfg.depth_every.clone(),
        }),
    );
    let bars = bars?;
    let depths = depths?;
    info!(bars = bars.len(), depths = depths.len(), "fetched");

    let events = merge_events(bars, depths);
    if events.is_empty() {
        return Ok(BacktestResult {
            portfolio: Portfolio::new(cfg.initial_equity, cfg.fee_bps),
            equity_curve: Vec::new(),
        });
    }

    let mut portfolio = Portfolio::new(cfg.initial_equity, cfg.fee_bps);
    let mut pending: Vec<BacktestOrder> = Vec::new();
    let mut equity_curve: Vec<EquityPoint> = Vec::with_capacity(events.len());
    let mut last_depth: Option<DepthSnapshot> = None;
    let depth_fresh_ns = cfg.depth_freshness_sec.saturating_mul(1_000_000_000);

    {
        let mut ctx = Context {
            now_ns: events[0].ts_ns(),
            position: portfolio.position.clone(),
            equity: portfolio.equity,
            pending: &mut pending,
        };
        strategy.on_start(&mut ctx);
    }

    for ev in &events {
        let ts = ev.ts_ns();

        // (1) 직전 tick의 pending orders 체결 — look-ahead 방지
        let buy_price = pick_buy_price(ev, &last_depth, ts, depth_fresh_ns);
        let sell_price = pick_sell_price(ev, &last_depth, ts, depth_fresh_ns);
        let drained: Vec<BacktestOrder> = pending.drain(..).collect();
        for order in drained {
            let price = match &order {
                BacktestOrder::Market {
                    side: Side::Long, ..
                } => buy_price,
                BacktestOrder::Market {
                    side: Side::Short, ..
                } => sell_price,
                BacktestOrder::Close => match portfolio.position.side {
                    Some(Side::Long) => sell_price,
                    Some(Side::Short) => buy_price,
                    None => continue,
                },
            };
            portfolio.execute(&order, price, ts);
        }

        // (2) mark-to-market
        let mark = event_mark_price(ev);
        portfolio.mark(mark);
        equity_curve.push(EquityPoint {
            ts_ns: ts,
            equity: portfolio.equity,
            position_qty: portfolio.position.qty,
            mark_price: mark,
        });

        // (3) strategy hook
        let mut ctx = Context {
            now_ns: ts,
            position: portfolio.position.clone(),
            equity: portfolio.equity,
            pending: &mut pending,
        };
        match ev {
            Event::Bar(b) => strategy.on_bar(b, &mut ctx),
            Event::Depth(d) => {
                strategy.on_depth(d, &mut ctx);
                last_depth = Some(d.clone());
            }
        }
        if let Event::Bar(_) = ev {
            // depth는 마지막 이벤트가 bar여도 직전 depth를 유지
        }
    }

    // 종료 시 잔여 포지션 시장가 청산 (마지막 이벤트 가격으로)
    if portfolio.position.side.is_some() {
        let last = events.last().unwrap();
        let close_price = event_mark_price(last);
        portfolio.execute(&BacktestOrder::Close, close_price, last.ts_ns());
        // equity_curve 마지막 행 갱신
        if let Some(p) = equity_curve.last_mut() {
            p.equity = portfolio.equity;
            p.position_qty = Decimal::zero();
        }
    }

    {
        let mut ctx = Context {
            now_ns: events.last().unwrap().ts_ns(),
            position: portfolio.position.clone(),
            equity: portfolio.equity,
            pending: &mut pending,
        };
        strategy.on_finish(&mut ctx);
    }

    Ok(BacktestResult {
        portfolio,
        equity_curve,
    })
}

fn merge_events(bars: Vec<Bar>, depths: Vec<DepthSnapshot>) -> Vec<Event> {
    let mut out = Vec::with_capacity(bars.len() + depths.len());
    let mut bi = 0;
    let mut di = 0;
    let mut bars = bars;
    let mut depths = depths;
    while bi < bars.len() && di < depths.len() {
        if bars[bi].ts_ns <= depths[di].ts_ns {
            out.push(Event::Bar(std::mem::replace(
                &mut bars[bi],
                Bar {
                    symbol: String::new(),
                    ts_ns: 0,
                    open: Decimal::zero(),
                    high: Decimal::zero(),
                    low: Decimal::zero(),
                    close: Decimal::zero(),
                    volume: Decimal::zero(),
                },
            )));
            bi += 1;
        } else {
            out.push(Event::Depth(std::mem::replace(
                &mut depths[di],
                DepthSnapshot {
                    symbol: String::new(),
                    ts_ns: 0,
                    bid1_price: Decimal::zero(),
                    bid1_qty: Decimal::zero(),
                    ask1_price: Decimal::zero(),
                    ask1_qty: Decimal::zero(),
                },
            )));
            di += 1;
        }
    }
    while bi < bars.len() {
        out.push(Event::Bar(std::mem::replace(
            &mut bars[bi],
            Bar {
                symbol: String::new(),
                ts_ns: 0,
                open: Decimal::zero(),
                high: Decimal::zero(),
                low: Decimal::zero(),
                close: Decimal::zero(),
                volume: Decimal::zero(),
            },
        )));
        bi += 1;
    }
    while di < depths.len() {
        out.push(Event::Depth(std::mem::replace(
            &mut depths[di],
            DepthSnapshot {
                symbol: String::new(),
                ts_ns: 0,
                bid1_price: Decimal::zero(),
                bid1_qty: Decimal::zero(),
                ask1_price: Decimal::zero(),
                ask1_qty: Decimal::zero(),
            },
        )));
        di += 1;
    }
    out
}

fn pick_buy_price(
    ev: &Event,
    last_depth: &Option<DepthSnapshot>,
    now_ns: i64,
    fresh_ns: i64,
) -> Decimal {
    if let Some(d) = last_depth
        && now_ns - d.ts_ns <= fresh_ns {
        return d.ask1_price;
    }
    match ev {
        Event::Bar(b) => b.open,
        Event::Depth(d) => d.ask1_price,
    }
}

fn pick_sell_price(
    ev: &Event,
    last_depth: &Option<DepthSnapshot>,
    now_ns: i64,
    fresh_ns: i64,
) -> Decimal {
    if let Some(d) = last_depth
        && now_ns - d.ts_ns <= fresh_ns {
        return d.bid1_price;
    }
    match ev {
        Event::Bar(b) => b.open,
        Event::Depth(d) => d.bid1_price,
    }
}

fn event_mark_price(ev: &Event) -> Decimal {
    match ev {
        Event::Bar(b) => b.close,
        Event::Depth(d) => (d.bid1_price + d.ask1_price) / Decimal::from(2),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::types::Side;
    use rust_decimal_macros::dec;

    fn mk_bar(ts_ns: i64, open: Decimal, close: Decimal) -> Bar {
        Bar {
            symbol: "X".into(),
            ts_ns,
            open,
            high: close,
            low: open,
            close,
            volume: dec!(1),
        }
    }

    #[test]
    fn merge_orders_by_ts() {
        let bars = vec![mk_bar(1, dec!(100), dec!(101)), mk_bar(3, dec!(102), dec!(103))];
        let depths = vec![
            DepthSnapshot {
                symbol: "X".into(),
                ts_ns: 2,
                bid1_price: dec!(100),
                bid1_qty: dec!(1),
                ask1_price: dec!(101),
                ask1_qty: dec!(1),
            },
            DepthSnapshot {
                symbol: "X".into(),
                ts_ns: 4,
                bid1_price: dec!(102),
                bid1_qty: dec!(1),
                ask1_price: dec!(103),
                ask1_qty: dec!(1),
            },
        ];
        let merged = merge_events(bars, depths);
        let ts: Vec<i64> = merged.iter().map(|e| e.ts_ns()).collect();
        assert_eq!(ts, vec![1, 2, 3, 4]);
    }

    struct OneShotLong {
        fired: bool,
    }
    impl Strategy for OneShotLong {
        fn on_bar(&mut self, _bar: &Bar, ctx: &mut Context) {
            if !self.fired {
                ctx.submit_market(Side::Long, dec!(1));
                self.fired = true;
            }
        }
    }

    struct StaticSource {
        bars: Vec<Bar>,
    }
    #[async_trait::async_trait]
    impl MarketDataSource for StaticSource {
        async fn fetch_bars(&self, _q: BarQuery) -> Result<Vec<Bar>> {
            Ok(self.bars.clone())
        }
        async fn fetch_depth_snapshots(&self, _q: DepthQuery) -> Result<Vec<DepthSnapshot>> {
            Ok(vec![])
        }
    }

    #[tokio::test]
    async fn one_tick_delay_fills_at_next_bar_open() {
        // Bar1: open=100, close=110. Strategy submits Long on bar1.
        // Bar2: open=120, close=130. Fill must occur at 120 (next bar's open), not 110.
        let src = StaticSource {
            bars: vec![
                mk_bar(1_000_000_000, dec!(100), dec!(110)),
                mk_bar(2_000_000_000, dec!(120), dec!(130)),
                mk_bar(3_000_000_000, dec!(125), dec!(135)),
            ],
        };
        let cfg = BacktestConfig {
            symbol: "X".into(),
            from_ns: 0,
            to_ns: 10_000_000_000,
            bar_interval: "1s".into(),
            depth_every: "1s".into(),
            initial_equity: dec!(10000),
            fee_bps: dec!(0),
            depth_freshness_sec: 0,
        };
        let mut strat = OneShotLong { fired: false };
        let res = run(&src, cfg, &mut strat).await.unwrap();
        // 마지막에 자동 청산 — close 가격은 마지막 bar의 close=135
        let trade = &res.portfolio.closed_trades[0];
        assert_eq!(trade.entry_price, dec!(120));
        assert_eq!(trade.exit_price, dec!(135));
    }
}
