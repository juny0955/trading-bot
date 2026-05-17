use crate::backtest::data::{BarQuery, DepthQuery, MarketDataSource};
use crate::backtest::strategy::{Context, Strategy};
use crate::backtest::types::{Bar, DepthSnapshot, Event};
use crate::order::backtest::{BacktestOrderExecutor, PortfolioSnapshot};
use crate::order::executor::OrderExecutor;
use crate::order::types::{OrderRequest, OrderSide, OrderType};
use anyhow::Result;
use rust_decimal::Decimal;
use std::sync::Arc;
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
    pub portfolio: PortfolioSnapshot,
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
        let executor = BacktestOrderExecutor::new(cfg.initial_equity, cfg.fee_bps);
        return Ok(BacktestResult {
            portfolio: executor.snapshot(),
            equity_curve: Vec::new(),
        });
    }

    let executor = Arc::new(BacktestOrderExecutor::new(cfg.initial_equity, cfg.fee_bps));

    let mut equity_curve = Vec::with_capacity(events.len());
    let mut last_depth: Option<&DepthSnapshot> = None;
    let depth_fresh_ns = cfg.depth_freshness_sec.saturating_mul(1_000_000_000);

    {
        let ctx = Context {
            now_ns: events[0].ts_ns(),
            equity: executor.equity(),
            position: executor.position(),
            executor: executor.clone(),
        };
        strategy.on_start(&ctx).await;
    }

    for ev in &events {
        let ts = ev.ts_ns();
        let buy_price = pick_price(ev, last_depth, ts, depth_fresh_ns, OrderSide::Buy);
        let sell_price = pick_price(ev, last_depth, ts, depth_fresh_ns, OrderSide::Sell);
        executor.drain_pending(buy_price, sell_price, ts);

        let mark = event_mark_price(ev);
        executor.mark(mark);
        equity_curve.push(EquityPoint {
            ts_ns: ts,
            equity: executor.equity(),
            position_qty: executor.position().qty,
            mark_price: mark,
        });

        let ctx = Context {
            now_ns: ts,
            equity: executor.equity(),
            position: executor.position(),
            executor: executor.clone(),
        };
        match ev {
            Event::Bar(b) => strategy.on_bar(b, &ctx).await,
            Event::Depth(d) => {
                strategy.on_depth(d, &ctx).await;
                last_depth = Some(d);
            }
        }
    }

    // 종료 시 잔여 포지션 시장가 청산 (마지막 이벤트 가격으로)
    if let Some(cur_side) = executor.position().side {
        let last = &events[events.len() - 1];
        let close_price = event_mark_price(last);
        let close_side = match cur_side {
            OrderSide::Buy => OrderSide::Sell,
            OrderSide::Sell => OrderSide::Buy,
        };

        let qty = executor.position().qty;
        let _ = executor
            .submit(OrderRequest {
                symbol: cfg.symbol.clone(),
                order_type: OrderType::Market,
                side: close_side,
                qty,
                reduce_only: true,
                ..Default::default()
            })
            .await;
        executor.drain_pending(close_price, close_price, last.ts_ns());
        if let Some(p) = equity_curve.last_mut() {
            p.equity = executor.equity();
            p.position_qty = Decimal::ZERO;
        }
    }

    {
        let last = events.last().unwrap().ts_ns();
        let ctx = Context {
            now_ns: last,
            equity: executor.equity(),
            position: executor.position(),
            executor: executor.clone(),
        };
        strategy.on_finish(&ctx).await;
    }

    Ok(BacktestResult {
        portfolio: executor.snapshot(),
        equity_curve,
    })
}

fn pick_price(
    ev: &Event,
    last_depth: Option<&DepthSnapshot>,
    now_ns: i64,
    fresh_ns: i64,
    side: OrderSide,
) -> Decimal {
    let select = |d: &DepthSnapshot| match side {
        OrderSide::Buy => d.ask1_price,
        OrderSide::Sell => d.bid1_price,
    };
    if let Some(d) = last_depth.filter(|d| now_ns - d.ts_ns <= fresh_ns) {
        return select(d);
    }
    match ev {
        Event::Bar(b) => b.open,
        Event::Depth(d) => select(d),
    }
}

fn event_mark_price(ev: &Event) -> Decimal {
    match ev {
        Event::Bar(b) => b.close,
        Event::Depth(d) => (d.bid1_price + d.ask1_price) / Decimal::from(2),
    }
}

fn merge_events(bars: Vec<Bar>, depths: Vec<DepthSnapshot>) -> Vec<Event> {
    let mut out = Vec::with_capacity(bars.len() + depths.len());
    let mut bars = bars.into_iter().peekable();
    let mut depths = depths.into_iter().peekable();
    loop {
        match (bars.peek().map(|b| b.ts_ns), depths.peek().map(|d| d.ts_ns)) {
            (Some(bt), Some(dt)) if bt <= dt => out.push(Event::Bar(bars.next().unwrap())),
            (Some(_), Some(_)) => out.push(Event::Depth(depths.next().unwrap())),
            (Some(_), None) => out.push(Event::Bar(bars.next().unwrap())),
            (None, Some(_)) => out.push(Event::Depth(depths.next().unwrap())),
            (None, None) => break,
        }
    }
    out
}
