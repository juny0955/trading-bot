use crate::application::live_order_service::LiveOrderService;
use crate::domain::backtest::{Bar, DepthSnapshot, Position};
use crate::domain::event::MarketDataEvent;
use crate::domain::market_data::{Depth, Kline};
use crate::domain::order::OrderSide;
use crate::domain::strategies::{Context, Strategy, StrategyCommand};
use crate::ports::live_market_feed::LiveMarketFeed;
use anyhow::Result;
use chrono::Utc;
use rust_decimal::Decimal;
use std::sync::Arc;
use tokio::sync::{Mutex, mpsc};
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub async fn run(
    feed: Box<dyn LiveMarketFeed>,
    mut strategy: Box<dyn Strategy>,
    order_svc: Arc<LiveOrderService>,
    position: Arc<Mutex<Position>>,
    token: CancellationToken,
) -> Result<()> {
    let (market_tx, mut market_rx) = mpsc::channel::<MarketDataEvent>(256);

    let feed_token = token.clone();
    tokio::spawn(async move {
        feed.run(market_tx, feed_token).await;
    });

    let ctx = make_ctx(&position, now_ns()).await;
    let cmds = strategy.on_start(&ctx);
    dispatch(&order_svc, &position, cmds).await;

    loop {
        tokio::select! {
            _ = token.cancelled() => break,
            event = market_rx.recv() => match event {
                Some(e) => {
                    let ctx = make_ctx(&position, now_ns()).await;
                    let cmds = on_event(&mut *strategy, e, &ctx);
                    dispatch(&order_svc, &position, cmds).await;
                }
                None => {
                    error!("market feed 채널 닫힘");
                    break;
                }
            }
        }
    }

    let ctx = make_ctx(&position, now_ns()).await;
    let cmds = strategy.on_finish(&ctx);
    dispatch(&order_svc, &position, cmds).await;

    Ok(())
}

fn on_event(
    strategy: &mut dyn Strategy,
    event: MarketDataEvent,
    ctx: &Context,
) -> Vec<StrategyCommand> {
    match event {
        MarketDataEvent::Kline(k) if k.is_closed => strategy.on_bar(&kline_to_bar(k), ctx),
        MarketDataEvent::Depth(d) => depth_to_snapshot(d)
            .map(|s| strategy.on_depth(&s, ctx))
            .unwrap_or_default(),
        _ => vec![],
    }
}

async fn dispatch(
    svc: &Arc<LiveOrderService>,
    position: &Arc<Mutex<Position>>,
    cmds: Vec<StrategyCommand>,
) {
    for cmd in cmds {
        match cmd {
            StrategyCommand::SubmitOrder(req) => {
                let side = req.side;
                let qty = req.qty;
                let reduce_only = req.reduce_only;

                match svc.submit(req).await {
                    Ok(order) => {
                        info!(id = %order.id, ?side, %qty, "주문 제출");
                        update_position(position, side, qty, reduce_only).await;
                    }
                    Err(e) => error!("주문 실패: {e}"),
                }
            }
        }
    }
}

async fn update_position(
    position: &Arc<Mutex<Position>>,
    side: OrderSide,
    qty: Decimal,
    reduce_only: bool,
) {
    let mut p = position.lock().await;
    if reduce_only {
        p.qty = (p.qty - qty).max(Decimal::ZERO);
        if p.qty == Decimal::ZERO {
            *p = Position::default();
        }
    } else {
        p.side = Some(side);
        p.qty = qty;
        // entry_price: 실제 체결가는 AccountEventHandler에서 확정
    }
}

async fn make_ctx(position: &Arc<Mutex<Position>>, now_ns: i64) -> Context {
    Context {
        now_ns,
        equity: Decimal::ZERO,
        position: position.lock().await.clone(),
    }
}

fn now_ns() -> i64 {
    Utc::now().timestamp_nanos_opt().unwrap_or(0)
}

fn kline_to_bar(k: Kline) -> Bar {
    Bar {
        symbol: k.symbol,
        ts_ns: k.open_time_ms * 1_000_000,
        open: k.open,
        high: k.high,
        low: k.low,
        close: k.close,
        volume: k.volume,
    }
}

fn depth_to_snapshot(d: Depth) -> Option<DepthSnapshot> {
    let bid = d.bids.first()?;
    let ask = d.asks.first()?;

    Some(DepthSnapshot {
        symbol: d.symbol,
        ts_ns: d.event_time_ns,
        bid1_price: bid.price,
        bid1_qty: bid.quantity,
        ask1_price: ask.price,
        ask1_qty: ask.quantity,
    })
}
