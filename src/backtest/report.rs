use crate::backtest::engine::{BacktestResult, EquityPoint};
use crate::backtest::portfolio::ClosedTrade;
use crate::backtest::types::Side;
use anyhow::{Context, Result};
use rust_decimal::Decimal;
use rust_decimal::prelude::Zero;
use serde::Serialize;
use std::fs::File;
use std::path::Path;

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

#[derive(Serialize)]
struct Summary {
    initial_equity: Decimal,
    final_equity: Decimal,
    total_pnl: Decimal,
    total_pnl_pct: Decimal,
    num_trades: usize,
    win_rate: Decimal,
    max_drawdown_pct: Decimal,
}

fn write_summary(out_dir: &Path, result: &BacktestResult) -> Result<()> {
    let initial = result.portfolio.initial_equity;
    let final_eq = result.portfolio.equity;
    let total_pnl = final_eq - initial;
    let total_pnl_pct = if initial.is_zero() {
        Decimal::zero()
    } else {
        total_pnl / initial * Decimal::from(100)
    };
    let num_trades = result.portfolio.closed_trades.len();
    let wins = result
        .portfolio
        .closed_trades
        .iter()
        .filter(|t| t.net_pnl > Decimal::zero())
        .count();
    let win_rate = if num_trades == 0 {
        Decimal::zero()
    } else {
        Decimal::from(wins) / Decimal::from(num_trades)
    };
    let max_dd = max_drawdown_pct(&result.equity_curve);

    let summary = Summary {
        initial_equity: initial,
        final_equity: final_eq,
        total_pnl,
        total_pnl_pct,
        num_trades,
        win_rate,
        max_drawdown_pct: max_dd,
    };
    let f = File::create(out_dir.join("summary.json"))?;
    serde_json::to_writer_pretty(f, &summary)?;
    Ok(())
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

fn side_str(s: Side) -> &'static str {
    match s {
        Side::Long => "long",
        Side::Short => "short",
    }
}
