use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use trading_bot::backtest::data::{BarQuery, DepthQuery, MarketDataSource};
use trading_bot::backtest::engine::{self, BacktestConfig};
use trading_bot::backtest::strategy::{Context, Strategy};
use trading_bot::backtest::types::{Bar, DepthSnapshot};
use trading_bot::order::executor::OrderExecutor;
use trading_bot::order::types::{OrderRequest, OrderSide, OrderType};

struct LinearSource {
    bars: Vec<Bar>,
}

#[async_trait]
impl MarketDataSource for LinearSource {
    async fn fetch_bars(&self, _q: BarQuery) -> Result<Vec<Bar>> {
        Ok(self.bars.clone())
    }
    async fn fetch_depth_snapshots(&self, _q: DepthQuery) -> Result<Vec<DepthSnapshot>> {
        Ok(vec![])
    }
}

struct BuyAndHold {
    fired: bool,
    qty: Decimal,
}

#[async_trait]
impl Strategy for BuyAndHold {
    async fn on_bar(&mut self, bar: &Bar, ctx: &Context) {
        if !self.fired {
            let _ = ctx
                .executor
                .submit(OrderRequest {
                    symbol: bar.symbol.clone(),
                    order_type: OrderType::Market,
                    side: OrderSide::Buy,
                    qty: self.qty,
                    ..Default::default()
                })
                .await;
            self.fired = true;
        }
    }
}

#[tokio::test]
async fn buy_and_hold_golden_pnl() {
    // 가격 100 → 200 단조 증가 (10 bars, step=11.111...)
    // 첫 bar 에서 Long 진입 → 두 번째 bar open = 111.11... 에서 fill
    // 끝에서 자동 청산 → 마지막 bar close = 200 에서 fill
    // 의도: 실제 entry/exit 가격 위에서 계산되는 PnL이 fee 식과 정확히 일치하는지 확인
    let n: i64 = 10;
    let bars: Vec<Bar> = (0..n)
        .map(|i| {
            let step = Decimal::from(i) * dec!(100) / Decimal::from(n - 1);
            let close = dec!(100) + step;
            Bar {
                symbol: "X".into(),
                ts_ns: (i as i64 + 1) * 1_000_000_000,
                open: close,
                high: close,
                low: close,
                close,
                volume: dec!(1),
            }
        })
        .collect();

    let src = LinearSource { bars: bars.clone() };
    let cfg = BacktestConfig {
        symbol: "X".into(),
        from_ns: 0,
        to_ns: 1_000_000_000_000,
        bar_interval: "1s".into(),
        depth_every: "1s".into(),
        initial_equity: dec!(10000),
        fee_bps: dec!(4),
        depth_freshness_sec: 0,
    };
    let mut strat = BuyAndHold {
        fired: false,
        qty: dec!(1),
    };
    let res = engine::run(&src, cfg, &mut strat).await.unwrap();

    assert_eq!(res.portfolio.closed_trades.len(), 1);
    let trade = &res.portfolio.closed_trades[0];
    let entry = bars[1].open;
    let exit = bars.last().unwrap().close;
    assert_eq!(trade.entry_price, entry);
    assert_eq!(trade.exit_price, exit);

    let expected_gross = (exit - entry) * dec!(1);
    let expected_fees = (entry + exit) * dec!(1) * dec!(4) / dec!(10000);
    let expected_net = expected_gross - expected_fees;
    assert_eq!(trade.gross_pnl, expected_gross);
    assert_eq!(trade.fees, expected_fees);
    assert_eq!(trade.net_pnl, expected_net);

    assert_eq!(res.portfolio.equity, dec!(10000) + expected_net);
}
