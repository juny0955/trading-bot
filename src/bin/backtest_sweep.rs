use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, NaiveDate, TimeZone, Utc};
use clap::Parser;
use futures_util::future::join_all;
use rust_decimal::Decimal;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::info;
use trading_bot::adapters::outbound::questdb_rest::data_source::QuestDbRestDataSource;
use trading_bot::application::backtest::engine::{self, BacktestConfig};
use trading_bot::application::backtest::report;
use trading_bot::domain::strategies::create_strategy;
use trading_bot::init;
use trading_bot::ports::market_data_source::{BarInterval, DepthSampleInterval};

#[derive(Parser, Debug)]
#[command(name = "backtest_sweep")]
struct Args {
    #[arg(long)]
    symbol: String,
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
    #[arg(long, default_value = "data/sweep_results")]
    output_dir: String,

    /// 파라미터 스윕: 단일 전략 이름 (--params-list 와 함께 사용)
    #[arg(long, requires = "params_list")]
    strategy: Option<String>,
    /// 파라미터 스윕: JSON 배열. 예: '[{"fast":9,"slow":21,"qty":"0.001"},...]'
    #[arg(long, requires = "strategy")]
    params_list: Option<String>,

    /// 멀티 전략 비교: JSON 배열. 예: '[{"name":"ema_cross","params":{...}},...]'
    #[arg(long, conflicts_with_all = ["strategy", "params_list"])]
    strategies: Option<String>,
}

#[derive(Deserialize)]
struct StrategySpec {
    name: String,
    params: serde_json::Value,
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

    let bar_interval = args
        .bar_interval
        .parse::<BarInterval>()
        .map_err(|e| anyhow!("--bar-interval 파싱 실패: {e}"))?;
    let depth_every = args
        .depth_every
        .parse::<DepthSampleInterval>()
        .map_err(|e| anyhow!("--depth-every 파싱 실패: {e}"))?;

    let base_cfg = BacktestConfig {
        symbol: args.symbol.clone(),
        from_ns,
        to_ns,
        bar_interval,
        depth_every,
        initial_equity: args.initial_equity,
        fee_bps: args.fee_bps,
        depth_freshness_sec: args.depth_freshness_sec,
    };

    let specs: Vec<StrategySpec> = if let Some(s) = &args.strategies {
        serde_json::from_str(s).context("--strategies JSON 파싱 실패")?
    } else {
        let name = args
            .strategy
            .as_ref()
            .ok_or_else(|| anyhow!("--strategy 또는 --strategies 중 하나가 필요합니다"))?;
        let list: Vec<serde_json::Value> = serde_json::from_str(args.params_list.as_ref().unwrap())
            .context("--params-list JSON 파싱 실패")?;
        list.into_iter()
            .map(|p| StrategySpec {
                name: name.clone(),
                params: p,
            })
            .collect()
    };

    if specs.is_empty() {
        return Err(anyhow!("스윕할 전략이 없습니다"));
    }

    info!("이벤트 fetch 중...");
    let events = Arc::new(engine::fetch_events(&src, &base_cfg).await?);
    info!(
        count = events.len(),
        "이벤트 fetch 완료, {} 개 전략 병렬 실행",
        specs.len()
    );

    let out_root = PathBuf::from(&args.output_dir);
    std::fs::create_dir_all(&out_root).context("출력 디렉토리 생성 실패")?;

    let handles: Vec<_> = specs
        .into_iter()
        .enumerate()
        .map(|(i, spec)| {
            let events = Arc::clone(&events);
            let cfg = base_cfg.clone();
            tokio::spawn(async move {
                let strategy = create_strategy(&spec.name, &spec.params)?;
                let result = engine::run_on_events(events, cfg, strategy).await?;
                anyhow::Ok((i, spec, result))
            })
        })
        .collect();

    let join_results = join_all(handles).await;

    let mut rows: Vec<(usize, String, String, report::Summary)> = Vec::new();
    for res in join_results {
        let (i, spec, result) = res.context("tokio task 패닉")??;
        let out_dir = out_root.join(format!("{i:03}_{}", spec.name));
        report::write_all(&result, &out_dir, &spec.params)?;
        let summary = report::build_summary(&result);
        info!(
            run = i,
            strategy = %spec.name,
            pnl_pct = %summary.total_pnl_pct,
            trades = summary.num_trades,
            "완료"
        );
        rows.push((i, spec.name, spec.params.to_string(), summary));
    }

    write_comparison_csv(&out_root, &rows)?;
    info!(output = %out_root.display(), "스윕 완료");
    Ok(())
}

fn write_comparison_csv(
    out_root: &Path,
    rows: &[(usize, String, String, report::Summary)],
) -> Result<()> {
    let mut w = csv::Writer::from_path(out_root.join("comparison.csv"))?;
    w.write_record([
        "run_id",
        "strategy",
        "params",
        "total_pnl_pct",
        "sharpe_ratio",
        "max_drawdown_pct",
        "win_rate",
        "profit_factor",
        "gross_profit_factor",
        "total_fees",
        "avg_fee_bps",
        "num_trades",
    ])?;
    for (i, name, params, s) in rows {
        w.write_record([
            i.to_string(),
            name.clone(),
            params.clone(),
            s.total_pnl_pct.to_string(),
            s.sharpe_ratio.map_or(String::new(), |v| v.to_string()),
            s.max_drawdown_pct.to_string(),
            s.win_rate.to_string(),
            s.profit_factor.map_or(String::new(), |v| v.to_string()),
            s.gross_profit_factor.map_or(String::new(), |v| v.to_string()),
            s.total_fees.to_string(),
            s.avg_fee_bps.to_string(),
            s.num_trades.to_string(),
        ])?;
    }
    w.flush()?;
    Ok(())
}

fn parse_date_to_ns(s: &str) -> Result<i64> {
    let date = NaiveDate::parse_from_str(s, "%Y-%m-%d")?;
    let naive = date.and_hms_opt(0, 0, 0).unwrap();
    let dt: DateTime<Utc> = Utc.from_utc_datetime(&naive);
    dt.timestamp_nanos_opt()
        .ok_or_else(|| anyhow!("timestamp ns overflow"))
}
