use crate::application::backtest::backtest_executor::{BacktestOrderExecutor, PortfolioSnapshot};
use crate::domain::backtest::{Bar, DepthSnapshot, Event};
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use crate::ports::market_data_source::{
    BarInterval, BarQuery, DepthQuery, DepthSampleInterval, MarketDataSource,
};
use crate::ports::order_executor::OrderExecutor;
use anyhow::Result;
use rust_decimal::Decimal;
use std::sync::Arc;
use tracing::info;

#[derive(Clone)]
pub struct BacktestConfig {
    pub symbol: String,
    pub from_ns: i64,
    pub to_ns: i64,
    pub bar_interval: BarInterval,
    pub depth_every: DepthSampleInterval,
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

pub async fn fetch_events(src: &dyn MarketDataSource, cfg: &BacktestConfig) -> Result<Vec<Event>> {
    let (bars, depths) = tokio::join!(
        src.fetch_bars(BarQuery {
            symbol: cfg.symbol.clone(),
            from_ns: cfg.from_ns,
            to_ns: cfg.to_ns,
            interval: cfg.bar_interval,
        }),
        src.fetch_depth_snapshots(DepthQuery {
            symbol: cfg.symbol.clone(),
            from_ns: cfg.from_ns,
            to_ns: cfg.to_ns,
            every: cfg.depth_every,
        }),
    );
    let bars = bars?;
    let depths = depths?;
    info!(bars = bars.len(), depths = depths.len(), "fetched");
    Ok(merge_events(bars, depths))
}

async fn run_inner(
    events: &[Event],
    cfg: &BacktestConfig,
    strategy: &mut dyn Strategy,
) -> Result<BacktestResult> {
    let executor = Arc::new(BacktestOrderExecutor::new(cfg.initial_equity, cfg.fee_bps));

    if events.is_empty() {
        return Ok(BacktestResult {
            portfolio: executor.snapshot(),
            equity_curve: Vec::new(),
        });
    }

    let mut equity_curve = Vec::with_capacity(events.len());
    let mut last_depth: Option<&DepthSnapshot> = None;
    let depth_fresh_ns = cfg.depth_freshness_sec.saturating_mul(1_000_000_000);

    {
        let ctx = make_ctx(&executor, events[0].ts_ns());
        let cmds = strategy.on_start(&ctx)?;
        dispatch(&executor, cmds).await;
    }

    for ev in events {
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

        let ctx = make_ctx(&executor, ts);
        let cmds = match ev {
            Event::Bar(b) => strategy.on_bar(b, &ctx)?,
            Event::Depth(d) => {
                last_depth = Some(d);
                strategy.on_depth(d, &ctx)?
            }
        };
        dispatch(&executor, cmds).await;
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
        let ctx = make_ctx(&executor, last);
        let cmds = strategy.on_finish(&ctx)?;
        dispatch(&executor, cmds).await;
    }

    Ok(BacktestResult {
        portfolio: executor.snapshot(),
        equity_curve,
    })
}

pub async fn run(
    src: &dyn MarketDataSource,
    cfg: BacktestConfig,
    strategy: &mut dyn Strategy,
) -> Result<BacktestResult> {
    let events = fetch_events(src, &cfg).await?;
    run_inner(&events, &cfg, strategy).await
}

pub async fn run_on_events(
    events: Arc<Vec<Event>>,
    cfg: BacktestConfig,
    mut strategy: Box<dyn Strategy>,
) -> Result<BacktestResult> {
    run_inner(&events, &cfg, strategy.as_mut()).await
}

fn make_ctx(executor: &Arc<BacktestOrderExecutor>, ts_ns: i64) -> Context {
    Context {
        now_ns: ts_ns,
        equity: executor.equity(),
        position: executor.position(),
    }
}

async fn dispatch(executor: &Arc<BacktestOrderExecutor>, cmds: Vec<StrategyCommand>) {
    for cmd in cmds {
        match cmd {
            StrategyCommand::SubmitOrder(req) => {
                let _ = executor.submit(req).await;
            }
        }
    }
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
    let mut bars_it = bars.into_iter();
    let mut depths_it = depths.into_iter();

    let mut next_bar = bars_it.next();
    let mut next_depth = depths_it.next();

    while next_bar.is_some() || next_depth.is_some() {
        match (&next_bar, &next_depth) {
            (Some(b), Some(d)) if b.ts_ns <= d.ts_ns => {
                out.push(Event::Bar(next_bar.take().unwrap()));
                next_bar = bars_it.next();
            }
            (None, Some(_)) | (Some(_), Some(_)) => {
                out.push(Event::Depth(next_depth.take().unwrap()));
                next_depth = depths_it.next();
            }
            (Some(_), None) => {
                out.push(Event::Bar(next_bar.take().unwrap()));
                next_bar = bars_it.next();
            }
            (None, None) => unreachable!(),
        }
    }
    out
}
