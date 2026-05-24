use crate::domain::backtest::{Bar, DepthSnapshot};
use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::position::Position;
use crate::domain::strategies::ema_cross::EmaCross;
use crate::domain::strategies::macd_cross::MacdCross;
use crate::domain::strategies::rsi_bounce::RsiBounce;
use anyhow::Result;
use rust_decimal::Decimal;
use serde_json::Value;

pub mod ema_cross;
pub mod macd_cross;
pub mod rsi_bounce;

/// 전략 실행 시점의 계좌/시장 컨텍스트입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::position::Position;
/// use trading_bot::domain::strategies::Context;
///
/// let ctx = Context {
///     now_ns: 1_000,
///     equity: dec!(10000),
///     position: Position::default(),
/// };
/// assert_eq!(ctx.now_ns, 1_000);
/// ```
pub struct Context {
    pub now_ns: i64,
    pub equity: Decimal,
    pub position: Position,
}

/// 전략이 실행 계층에 요청하는 명령입니다.
///
/// # Examples
///
/// ```
/// use rust_decimal_macros::dec;
/// use trading_bot::domain::order::{OrderRequest, OrderSide, OrderType};
/// use trading_bot::domain::strategies::StrategyCommand;
///
/// let command = StrategyCommand::SubmitOrder(OrderRequest {
///     symbol: "BTCUSDT".into(),
///     order_type: OrderType::Market,
///     side: OrderSide::Buy,
///     qty: dec!(1),
///     ..Default::default()
/// });
/// assert!(matches!(command, StrategyCommand::SubmitOrder(_)));
/// ```
pub enum StrategyCommand {
    SubmitOrder(OrderRequest),
}

/// 실시간 및 백테스트 엔진에서 호출하는 전략 인터페이스입니다.
///
/// # Examples
///
/// ```
/// use anyhow::Result;
/// use trading_bot::domain::backtest::Bar;
/// use trading_bot::domain::strategies::{Context, Strategy, StrategyCommand};
///
/// struct Noop;
///
/// impl Strategy for Noop {
///     fn on_bar(&mut self, _bar: &Bar, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
///         Ok(vec![])
///     }
/// }
/// ```
pub trait Strategy: Send {
    fn on_start(&mut self, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
        Ok(vec![])
    }

    fn on_bar(&mut self, _bar: &Bar, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
        Ok(vec![])
    }

    fn on_depth(&mut self, _snap: &DepthSnapshot, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
        Ok(vec![])
    }

    fn on_finish(&mut self, _ctx: &Context) -> Result<Vec<StrategyCommand>> {
        Ok(vec![])
    }

    fn close_and_open(
        position: &Position,
        symbol: &str,
        new_side: OrderSide,
        qty: Decimal,
    ) -> Vec<StrategyCommand>
    where
        Self: Sized,
    {
        let mut cmds = close_position_command(position, symbol);
        cmds.push(open_position_command(symbol, new_side, qty));
        cmds
    }
}

fn close_position_command(position: &Position, symbol: &str) -> Vec<StrategyCommand> {
    position
        .side
        .map(|side| {
            StrategyCommand::SubmitOrder(market_order(
                symbol,
                opposite_side(side),
                position.qty,
                true,
            ))
        })
        .into_iter()
        .collect()
}

fn open_position_command(symbol: &str, side: OrderSide, qty: Decimal) -> StrategyCommand {
    StrategyCommand::SubmitOrder(market_order(symbol, side, qty, false))
}

fn market_order(symbol: &str, side: OrderSide, qty: Decimal, reduce_only: bool) -> OrderRequest {
    OrderRequest {
        symbol: symbol.into(),
        order_type: OrderType::Market,
        side,
        qty,
        reduce_only,
        ..Default::default()
    }
}

fn opposite_side(side: OrderSide) -> OrderSide {
    match side {
        OrderSide::Buy => OrderSide::Sell,
        OrderSide::Sell => OrderSide::Buy,
    }
}

/// 저장소에서 읽은 전략 인스턴스 설정입니다.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use trading_bot::domain::strategies::StrategyInstance;
///
/// let instance = StrategyInstance {
///     id: 1,
///     strategy_type: "ema_cross".into(),
///     name: "EMA".into(),
///     params: json!({"fast": 9, "slow": 21, "qty": "0.001"}),
///     symbols: vec!["BTCUSDT".into()],
/// };
/// assert_eq!(instance.symbols.len(), 1);
/// ```
#[derive(Debug, Clone)]
pub struct StrategyInstance {
    pub id: i64,
    pub strategy_type: String,
    pub name: String,
    pub params: Value,
    pub symbols: Vec<String>,
}

/// 이름과 JSON 파라미터로 전략 객체를 생성합니다.
///
/// # Examples
///
/// ```
/// use serde_json::json;
/// use trading_bot::domain::strategies::create_strategy;
///
/// let params = json!({"fast": 9, "slow": 21, "qty": "0.001"});
/// let strategy = create_strategy("ema_cross", &params);
/// assert!(strategy.is_ok());
/// ```
pub fn create_strategy(name: &str, params: &Value) -> Result<Box<dyn Strategy>> {
    match name {
        "ema_cross" => Ok(Box::new(EmaCross::from_params(params)?)),
        "rsi_bounce" => Ok(Box::new(RsiBounce::from_params(params)?)),
        "macd_cross" => Ok(Box::new(MacdCross::from_params(params)?)),
        other => anyhow::bail!("알 수 없는 전략: {other}"),
    }
}
