use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Local, NaiveDate, TimeZone, Utc};
use clap::Parser;
use rust_decimal::Decimal;
use std::path::PathBuf;
use tracing::info;
use trading_bot::adapters::questdb_rest::data_source::QuestDbRestDataSource;
use trading_bot::application::backtest::engine::{self, BacktestConfig};
use trading_bot::application::backtest::report;
use trading_bot::domain::strategies;
use trading_bot::init;

#[derive(Parser, Debug, serde::Serialize)]
#[command(name = "backtest")]
struct Args {
    #[arg(long)]
    strategy: String,
    #[arg(long)]
    symbol: String,
    /// "YYYY-MM-DD" UTC 자정 기준
    #[arg(long)]
    from: String,
    #[arg(long)]
    to: String,
    #[arg(long, default_value = "1m")]
    bar_interval: String,
    #[arg(long, default_value = "5s")]
    depth_every: String,
    #[arg(long, default_value = "10000")]
    initial_equity: Decimal,
    #[arg(long, default_value = "4")]
    fee_bps: Decimal,
    #[arg(long, default_value = "10")]
    depth_freshness_sec: i64,
    /// strategy 파라미터 JSON. 예: '{"fast":12,"slow":26,"qty":0.001}'
    #[arg(long, default_value = "{}")]
    params: String,
    #[arg(long, default_value = "data/backtest_results")]
    output_dir: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    init::setup();
    let args = Args::parse();

    let rest_url =
        std::env::var("QUESTDB_REST_URL").unwrap_or_else(|_| "http://localhost:9000".to_string());
    info!(rest_url = %rest_url, "QuestDB REST 사용");

    let src = QuestDbRestDataSource::new(rest_url);

    let from_ns = parse_date_to_ns(&args.from).context("--from 파싱 실패")?;
    let to_ns = parse_date_to_ns(&args.to).context("--to 파싱 실패")?;
    if to_ns <= from_ns {
        return Err(anyhow!("--to 가 --from 보다 같거나 이전"));
    }

    let params: serde_json::Value =
        serde_json::from_str(&args.params).context("--params JSON 파싱 실패")?;
    let mut strategy = strategies::create_strategy(&args.strategy, &params)?;

    let cfg = BacktestConfig {
        symbol: args.symbol.clone(),
        from_ns,
        to_ns,
        bar_interval: args.bar_interval.clone(),
        depth_every: args.depth_every.clone(),
        initial_equity: args.initial_equity,
        fee_bps: args.fee_bps,
        depth_freshness_sec: args.depth_freshness_sec,
    };
    let result = engine::run(&src, cfg, strategy.as_mut()).await?;

    let out_dir = make_out_dir(&args.output_dir, &args.strategy, &args.symbol);
    let invocation = serde_json::to_value(&args)?;
    report::write_all(&result, &out_dir, &invocation)?;
    info!(
        out_dir = %out_dir.display(),
        initial = %result.portfolio.initial_equity,
        final = %result.portfolio.equity,
        trades = result.portfolio.closed_trades.len(),
        "백테스트 완료"
    );
    Ok(())
}

fn parse_date_to_ns(s: &str) -> Result<i64> {
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
    let naive = date.and_hms_opt(0, 0, 0).unwrap();
    let dt: DateTime<Utc> = Utc.from_utc_datetime(&naive);
    dt.timestamp_nanos_opt()
        .ok_or_else(|| anyhow!("timestamp ns overflow"))
}

fn make_out_dir(root: &str, strategy: &str, symbol: &str) -> PathBuf {
    let stamp = Local::now().format("%Y%m%d_%H%M%S").to_string();
    PathBuf::from(root).join(format!("{stamp}_{strategy}_{symbol}"))
}
