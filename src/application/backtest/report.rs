use crate::application::backtest::engine::{BacktestResult, EquityPoint};
use crate::application::backtest::portfolio::ClosedTrade;
use crate::domain::order::OrderSide;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use rust_decimal::prelude::{ToPrimitive, Zero};
use serde::Serialize;
use std::collections::BTreeMap;
use std::fs::File;
use std::path::Path;

/// 백테스트 결과 파일 전체를 출력합니다.
///
/// # Examples
///
/// ```ignore
/// # use std::path::Path;
/// # use trading_bot::application::backtest::engine::BacktestResult;
/// # fn example(result: &BacktestResult) -> anyhow::Result<()> {
/// let invocation = serde_json::json!({ "symbol": "BTCUSDT" });
/// trading_bot::application::backtest::report::write_all(
///     result,
///     Path::new("out/backtest"),
///     &invocation,
/// )?;
/// # Ok(()) }
/// ```
pub fn write_all(
    result: &BacktestResult,
    out_dir: &Path,
    invocation: &serde_json::Value,
) -> Result<()> {
    std::fs::create_dir_all(out_dir).context("결과 디렉토리 생성 실패")?;
    write_config(out_dir, invocation)?;
    write_trades(out_dir, &result.portfolio.closed_trades)?;
    write_equity(out_dir, &result.equity_curve)?;
    write_summary(out_dir, result)?;
    Ok(())
}

fn write_config(out_dir: &Path, invocation: &serde_json::Value) -> Result<()> {
    let f = File::create(out_dir.join("config.json"))?;
    serde_json::to_writer_pretty(f, invocation)?;
    Ok(())
}

fn write_trades(out_dir: &Path, trades: &[ClosedTrade]) -> Result<()> {
    let mut w = csv::Writer::from_path(out_dir.join("trades.csv"))?;
    w.write_record([
        "entry_ts_ns",
        "exit_ts_ns",
        "side",
        "qty",
        "entry_price",
        "exit_price",
        "fees",
        "gross_pnl",
        "net_pnl",
    ])?;
    for t in trades {
        w.write_record([
            t.entry_ts_ns.to_string(),
            t.exit_ts_ns.to_string(),
            side_str(t.side).to_string(),
            t.qty.to_string(),
            t.entry_price.to_string(),
            t.exit_price.to_string(),
            t.fees.to_string(),
            t.gross_pnl.to_string(),
            t.net_pnl.to_string(),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn write_equity(out_dir: &Path, curve: &[EquityPoint]) -> Result<()> {
    let mut w = csv::Writer::from_path(out_dir.join("equity.csv"))?;
    w.write_record(["ts_ns", "equity", "position_qty", "mark_price"])?;
    for p in curve {
        w.write_record([
            p.ts_ns.to_string(),
            p.equity.to_string(),
            p.position_qty.to_string(),
            p.mark_price.to_string(),
        ])?;
    }
    w.flush()?;
    Ok(())
}

/// 백테스트 요약 지표입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::application::backtest::report::Summary;
///
/// let summary = Summary {
///     initial_equity: dec!(1000),
///     final_equity: dec!(1100),
///     total_pnl: dec!(100),
///     total_pnl_pct: dec!(10),
///     num_trades: 0,
///     win_rate: dec!(0),
///     max_drawdown_pct: dec!(0),
///     sharpe_ratio: None,
///     profit_factor: None,
///     gross_profit_factor: None,
///     total_fees: dec!(0),
///     avg_fee_bps: dec!(0),
///     avg_win: dec!(0),
///     avg_loss: dec!(0),
///     avg_trade_duration_s: dec!(0),
/// };
/// assert_eq!(summary.total_pnl_pct, dec!(10));
/// ```
#[derive(Serialize)]
pub struct Summary {
    pub initial_equity: Decimal,
    pub final_equity: Decimal,
    pub total_pnl: Decimal,
    pub total_pnl_pct: Decimal,
    pub num_trades: usize,
    pub win_rate: Decimal,
    pub max_drawdown_pct: Decimal,
    pub sharpe_ratio: Option<f64>,
    pub profit_factor: Option<Decimal>,
    pub gross_profit_factor: Option<Decimal>,
    pub total_fees: Decimal,
    pub avg_fee_bps: Decimal,
    pub avg_win: Decimal,
    pub avg_loss: Decimal,
    pub avg_trade_duration_s: Decimal,
}

/// 백테스트 결과에서 요약 지표를 계산합니다.
///
/// # Examples
///
/// ```ignore
/// # use trading_bot::application::backtest::engine::BacktestResult;
/// # fn example(result: &BacktestResult) {
/// let summary = trading_bot::application::backtest::report::build_summary(result);
/// assert_eq!(summary.num_trades, result.portfolio.closed_trades.len());
/// # }
/// ```
pub fn build_summary(result: &BacktestResult) -> Summary {
    let initial = result.portfolio.initial_equity;
    let final_eq = result.portfolio.equity;
    let total_pnl = final_eq - initial;
    let trades = &result.portfolio.closed_trades;
    let num_trades = trades.len();
    let (avg_win, avg_loss) = avg_win_loss(trades);

    Summary {
        initial_equity: initial,
        final_equity: final_eq,
        total_pnl,
        total_pnl_pct: pct_change(initial, final_eq),
        num_trades,
        win_rate: win_rate(trades),
        max_drawdown_pct: max_drawdown_pct(&result.equity_curve),
        sharpe_ratio: sharpe_ratio(&result.equity_curve),
        profit_factor: net_profit_factor(trades),
        gross_profit_factor: gross_profit_factor(trades),
        total_fees: total_fees(trades),
        avg_fee_bps: avg_fee_bps(trades),
        avg_win,
        avg_loss,
        avg_trade_duration_s: avg_duration_s(trades),
    }
}

fn write_summary(out_dir: &Path, result: &BacktestResult) -> Result<()> {
    let summary = build_summary(result);
    let f = File::create(out_dir.join("summary.json"))?;
    serde_json::to_writer_pretty(f, &summary)?;
    Ok(())
}

fn pct_change(initial: Decimal, final_value: Decimal) -> Decimal {
    if initial.is_zero() {
        Decimal::zero()
    } else {
        (final_value - initial) / initial * Decimal::from(100)
    }
}

fn win_rate(trades: &[ClosedTrade]) -> Decimal {
    if trades.is_empty() {
        return Decimal::zero();
    }

    let wins = trades
        .iter()
        .filter(|trade| trade.net_pnl > Decimal::zero())
        .count();
    Decimal::from(wins) / Decimal::from(trades.len())
}

fn max_drawdown_pct(curve: &[EquityPoint]) -> Decimal {
    let mut peak = Decimal::zero();
    let mut max_dd = Decimal::zero();
    for p in curve {
        if p.equity > peak {
            peak = p.equity;
        }
        if peak > Decimal::zero() {
            let dd = (peak - p.equity) / peak * Decimal::from(100);
            if dd > max_dd {
                max_dd = dd;
            }
        }
    }
    max_dd
}

fn sharpe_ratio(curve: &[EquityPoint]) -> Option<f64> {
    // 일별 마지막 equity 집계 (key = day index)
    let mut daily: BTreeMap<i64, f64> = BTreeMap::new();
    for p in curve {
        let day = p.ts_ns / 86_400_000_000_000;
        daily.insert(day, p.equity.to_f64()?);
    }
    let vals: Vec<f64> = daily.into_values().collect();
    if vals.len() < 2 {
        return None;
    }
    let returns: Vec<f64> = vals.windows(2).map(|w| (w[1] - w[0]) / w[0]).collect();
    let mean = returns.iter().sum::<f64>() / returns.len() as f64;
    let variance = returns.iter().map(|r| (r - mean).powi(2)).sum::<f64>() / returns.len() as f64;
    let std_dev = variance.sqrt();
    if std_dev == 0.0 {
        return None;
    }
    Some(mean / std_dev * (252f64).sqrt())
}

fn net_profit_factor(trades: &[ClosedTrade]) -> Option<Decimal> {
    let net_win: Decimal = trades
        .iter()
        .filter(|t| t.net_pnl > Decimal::zero())
        .map(|t| t.net_pnl)
        .sum();
    let net_loss: Decimal = trades
        .iter()
        .filter(|t| t.net_pnl < Decimal::zero())
        .map(|t| t.net_pnl.abs())
        .sum();
    if net_loss.is_zero() {
        return None;
    }
    Some(net_win / net_loss)
}

fn gross_profit_factor(trades: &[ClosedTrade]) -> Option<Decimal> {
    let gross_win: Decimal = trades
        .iter()
        .filter(|t| t.gross_pnl > Decimal::zero())
        .map(|t| t.gross_pnl)
        .sum();
    let gross_loss: Decimal = trades
        .iter()
        .filter(|t| t.gross_pnl < Decimal::zero())
        .map(|t| t.gross_pnl.abs())
        .sum();
    if gross_loss.is_zero() {
        return None;
    }
    Some(gross_win / gross_loss)
}

fn total_fees(trades: &[ClosedTrade]) -> Decimal {
    trades.iter().map(|t| t.fees).sum()
}

fn avg_fee_bps(trades: &[ClosedTrade]) -> Decimal {
    let notional: Decimal = trades
        .iter()
        .map(|t| (t.entry_price * t.qty) + (t.exit_price * t.qty))
        .sum();
    if notional.is_zero() {
        Decimal::zero()
    } else {
        total_fees(trades) / notional * Decimal::from(10_000)
    }
}

fn avg_win_loss(trades: &[ClosedTrade]) -> (Decimal, Decimal) {
    let wins: Vec<Decimal> = trades
        .iter()
        .filter(|t| t.net_pnl > Decimal::zero())
        .map(|t| t.net_pnl)
        .collect();
    let losses: Vec<Decimal> = trades
        .iter()
        .filter(|t| t.net_pnl < Decimal::zero())
        .map(|t| t.net_pnl.abs())
        .collect();
    let avg = |v: &[Decimal]| {
        if v.is_empty() {
            Decimal::zero()
        } else {
            v.iter().sum::<Decimal>() / Decimal::from(v.len())
        }
    };
    (avg(&wins), avg(&losses))
}

fn avg_duration_s(trades: &[ClosedTrade]) -> Decimal {
    if trades.is_empty() {
        return Decimal::zero();
    }
    let total_ns: i64 = trades.iter().map(|t| t.exit_ts_ns - t.entry_ts_ns).sum();
    Decimal::from(total_ns) / Decimal::from(trades.len()) / Decimal::from(1_000_000_000i64)
}

fn side_str(s: OrderSide) -> &'static str {
    match s {
        OrderSide::Buy => "long",
        OrderSide::Sell => "short",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::application::backtest::backtest_executor::PortfolioSnapshot;
    use crate::domain::position::Position;
    use rust_decimal_macros::dec;

    fn result_with_trade(net_pnl: Decimal) -> BacktestResult {
        BacktestResult {
            portfolio: PortfolioSnapshot {
                equity: dec!(1100),
                initial_equity: dec!(1000),
                position: Position::default(),
                closed_trades: vec![ClosedTrade {
                    entry_ts_ns: 0,
                    exit_ts_ns: 1_000_000_000,
                    side: OrderSide::Buy,
                    qty: dec!(1),
                    entry_price: dec!(100),
                    exit_price: dec!(110),
                    fees: dec!(1),
                    gross_pnl: dec!(10),
                    net_pnl,
                }],
            },
            equity_curve: vec![
                EquityPoint {
                    ts_ns: 0,
                    equity: dec!(1000),
                    position_qty: dec!(0),
                    mark_price: dec!(100),
                },
                EquityPoint {
                    ts_ns: 1,
                    equity: dec!(900),
                    position_qty: dec!(0),
                    mark_price: dec!(90),
                },
            ],
        }
    }

    fn result_with_trades(trades: Vec<ClosedTrade>) -> BacktestResult {
        BacktestResult {
            portfolio: PortfolioSnapshot {
                equity: dec!(1000),
                initial_equity: dec!(1000),
                position: Position::default(),
                closed_trades: trades,
            },
            equity_curve: vec![],
        }
    }

    #[test]
    fn build_summary_calculates_core_metrics() -> anyhow::Result<()> {
        let summary = build_summary(&result_with_trade(dec!(9)));
        assert_eq!(summary.total_pnl, dec!(100));
        assert_eq!(summary.total_pnl_pct, dec!(10.0));
        assert_eq!(summary.win_rate, dec!(1));
        assert_eq!(summary.max_drawdown_pct, dec!(10.0));
        assert_eq!(summary.avg_trade_duration_s, dec!(1));
        assert_eq!(summary.total_fees, dec!(1));
        Ok(())
    }

    #[test]
    fn build_summary_handles_losses() -> anyhow::Result<()> {
        let summary = build_summary(&result_with_trade(dec!(-3)));
        assert_eq!(summary.win_rate, dec!(0));
        assert_eq!(summary.avg_loss, dec!(3));
        Ok(())
    }

    #[test]
    fn profit_factor_uses_net_pnl_and_keeps_gross_metric() -> anyhow::Result<()> {
        let trades = vec![
            ClosedTrade {
                entry_ts_ns: 0,
                exit_ts_ns: 1,
                side: OrderSide::Buy,
                qty: dec!(1),
                entry_price: dec!(100),
                exit_price: dec!(110),
                fees: dec!(3),
                gross_pnl: dec!(10),
                net_pnl: dec!(7),
            },
            ClosedTrade {
                entry_ts_ns: 2,
                exit_ts_ns: 3,
                side: OrderSide::Buy,
                qty: dec!(1),
                entry_price: dec!(100),
                exit_price: dec!(94),
                fees: dec!(2),
                gross_pnl: dec!(-6),
                net_pnl: dec!(-8),
            },
        ];
        let summary = build_summary(&result_with_trades(trades));
        assert_eq!(summary.profit_factor, Some(dec!(0.875)));
        assert_eq!(summary.gross_profit_factor, Some(dec!(1.6666666666666666666666666667)));
        assert_eq!(summary.total_fees, dec!(5));
        assert_eq!(
            summary.avg_fee_bps,
            dec!(5) / dec!(404) * Decimal::from(10_000)
        );
        Ok(())
    }
}
