use anyhow::Result;
use async_trait::async_trait;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use trading_bot::backtest::data::{BarQuery, DepthQuery, MarketDataSource};
use trading_bot::backtest::engine::{self, BacktestConfig};
use trading_bot::backtest::strategy::{Context, Strategy};
use trading_bot::backtest::types::{Bar, DepthSnapshot, Side};

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

impl Strategy for BuyAndHold {
    fn on_bar(&mut self, _bar: &Bar, ctx: &mut Context) {
        if !self.fired {
            ctx.submit_market(Side::Long, self.qty);
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
            // open = close (단순화: bar 내부 변동 없음)
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
    let entry = bars[1].open; // 두 번째 bar의 open 에서 fill (1-tick 지연)
    let exit = bars.last().unwrap().close; // 마지막 bar close 에서 자동 청산
    assert_eq!(trade.entry_price, entry);
    assert_eq!(trade.exit_price, exit);

    // 검증: gross_pnl = (exit - entry) * qty
    let expected_gross = (exit - entry) * dec!(1);
    // 수수료: entry_fee + exit_fee = (entry + exit) * qty * 4/10000
    let expected_fees = (entry + exit) * dec!(1) * dec!(4) / dec!(10000);
    let expected_net = expected_gross - expected_fees;
    assert_eq!(trade.gross_pnl, expected_gross);
    assert_eq!(trade.fees, expected_fees);
    assert_eq!(trade.net_pnl, expected_net);

    // final_equity = initial + realized
    assert_eq!(res.portfolio.equity, dec!(10000) + expected_net);
}
