use crate::domain::order::{OrderRequest, OrderSide, OrderType};
use crate::domain::strategies::StrategyCommand;
use crate::ports::outbound::risk_config_source::RiskLimits;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

const BPS_DIVISOR: Decimal = Decimal::from_parts(10_000, 0, 0, false, 0);

#[derive(Debug, Clone, Copy)]
pub struct Quote {
    pub bid: Decimal,
    pub ask: Decimal,
}

#[derive(Debug, Clone, Copy)]
pub struct Bracket {
    pub stop_price: Decimal,
    pub take_profit_price: Decimal,
}

#[derive(Debug, Clone)]
pub struct FeeGuardReport {
    pub entry_price: Decimal,
    pub spread_bps: Decimal,
    pub gross_reward: Decimal,
    pub gross_risk: Decimal,
    pub total_cost: Decimal,
    pub net_reward: Decimal,
    pub net_risk: Decimal,
    pub net_reward_bps: Decimal,
    pub net_reward_risk: Decimal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeeGuardReject {
    MissingQuote,
    MissingBracket,
    InvalidPrice,
    SpreadTooWide,
    NonPositiveReward,
    NonPositiveRisk,
    RewardBpsTooLow,
    RewardRiskTooLow,
}

#[derive(Debug, Clone)]
pub struct FeeGuardDecision {
    pub report: FeeGuardReport,
}

#[derive(Debug, Clone)]
pub struct FeeGuardConfig {
    pub fee_bps: Decimal,
    pub estimated_slippage_bps: Decimal,
    pub max_spread_bps: Decimal,
    pub min_net_reward_risk: Decimal,
    pub min_net_reward_bps: Decimal,
    pub require_bracket_for_entry: bool,
}

impl From<&RiskLimits> for FeeGuardConfig {
    fn from(limits: &RiskLimits) -> Self {
        Self {
            fee_bps: limits.fee_bps,
            estimated_slippage_bps: limits.estimated_slippage_bps,
            max_spread_bps: limits.max_spread_bps,
            min_net_reward_risk: limits.min_net_reward_risk,
            min_net_reward_bps: limits.min_net_reward_bps,
            require_bracket_for_entry: limits.require_bracket_for_entry,
        }
    }
}

pub fn evaluate_entry(
    req: &OrderRequest,
    bracket: Option<Bracket>,
    quote: Option<Quote>,
    cfg: &FeeGuardConfig,
) -> Result<FeeGuardDecision, FeeGuardReject> {
    if req.reduce_only {
        return Err(FeeGuardReject::InvalidPrice);
    }

    let quote = quote.ok_or(FeeGuardReject::MissingQuote)?;
    let bracket = match bracket {
        Some(v) => v,
        None if cfg.require_bracket_for_entry => return Err(FeeGuardReject::MissingBracket),
        None => return Err(FeeGuardReject::MissingBracket),
    };

    validate_positive(quote.bid)?;
    validate_positive(quote.ask)?;
    validate_positive(bracket.stop_price)?;
    validate_positive(bracket.take_profit_price)?;

    let entry_price = estimate_entry_price(req, quote)?;
    let spread_bps = spread_bps(quote)?;
    if cfg.max_spread_bps > Decimal::ZERO && spread_bps > cfg.max_spread_bps {
        return Err(FeeGuardReject::SpreadTooWide);
    }

    let gross_reward = side_price_delta(req.side, entry_price, bracket.take_profit_price);
    let gross_risk = side_price_delta(req.side, bracket.stop_price, entry_price);
    if gross_reward <= Decimal::ZERO {
        return Err(FeeGuardReject::NonPositiveReward);
    }
    if gross_risk <= Decimal::ZERO {
        return Err(FeeGuardReject::NonPositiveRisk);
    }

    let round_trip_fee = entry_price * cfg.fee_bps * dec!(2) / BPS_DIVISOR;
    let round_trip_slippage = entry_price * cfg.estimated_slippage_bps * dec!(2) / BPS_DIVISOR;
    let spread_cost = entry_price * spread_bps / BPS_DIVISOR;
    let total_cost = round_trip_fee + round_trip_slippage + spread_cost;
    let net_reward = gross_reward - total_cost;
    let net_risk = gross_risk + total_cost;
    let net_reward_bps = net_reward / entry_price * BPS_DIVISOR;

    if net_reward <= Decimal::ZERO || net_reward_bps < cfg.min_net_reward_bps {
        return Err(FeeGuardReject::RewardBpsTooLow);
    }
    if net_risk <= Decimal::ZERO {
        return Err(FeeGuardReject::NonPositiveRisk);
    }
    let net_reward_risk = net_reward / net_risk;
    if net_reward_risk < cfg.min_net_reward_risk {
        return Err(FeeGuardReject::RewardRiskTooLow);
    }

    Ok(FeeGuardDecision {
        report: FeeGuardReport {
            entry_price,
            spread_bps,
            gross_reward,
            gross_risk,
            total_cost,
            net_reward,
            net_risk,
            net_reward_bps,
            net_reward_risk,
        },
    })
}

pub fn bracket_for(side: OrderSide, qty: Decimal, cmds: &[StrategyCommand]) -> Option<Bracket> {
    let exit_side = opposite_side(side);
    let mut sl = None;
    let mut tp = None;
    for cmd in cmds {
        match cmd {
            StrategyCommand::SetStopLoss {
                side,
                qty: cmd_qty,
                stop_price,
            } if *side == exit_side && *cmd_qty == qty => sl = Some(*stop_price),
            StrategyCommand::SetTakeProfit {
                side,
                qty: cmd_qty,
                stop_price,
            } if *side == exit_side && *cmd_qty == qty => tp = Some(*stop_price),
            _ => {}
        }
    }
    Some(Bracket {
        stop_price: sl?,
        take_profit_price: tp?,
    })
}

pub fn is_new_entry(req: &OrderRequest) -> bool {
    !req.reduce_only && matches!(req.order_type, OrderType::Market | OrderType::Limit)
}

fn estimate_entry_price(req: &OrderRequest, quote: Quote) -> Result<Decimal, FeeGuardReject> {
    let price = match req.order_type {
        OrderType::Limit => req.price.unwrap_or(match req.side {
            OrderSide::Buy => quote.ask,
            OrderSide::Sell => quote.bid,
        }),
        _ => match req.side {
            OrderSide::Buy => quote.ask,
            OrderSide::Sell => quote.bid,
        },
    };
    validate_positive(price)?;
    Ok(price)
}

fn spread_bps(quote: Quote) -> Result<Decimal, FeeGuardReject> {
    let mid = (quote.bid + quote.ask) / dec!(2);
    validate_positive(mid)?;
    Ok((quote.ask - quote.bid) / mid * BPS_DIVISOR)
}

fn side_price_delta(side: OrderSide, lower: Decimal, upper: Decimal) -> Decimal {
    match side {
        OrderSide::Buy => upper - lower,
        OrderSide::Sell => lower - upper,
    }
}

fn opposite_side(side: OrderSide) -> OrderSide {
    match side {
        OrderSide::Buy => OrderSide::Sell,
        OrderSide::Sell => OrderSide::Buy,
    }
}

fn validate_positive(value: Decimal) -> Result<(), FeeGuardReject> {
    if value > Decimal::ZERO {
        Ok(())
    } else {
        Err(FeeGuardReject::InvalidPrice)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::order::{OrderRequest, OrderType};
    use rust_decimal_macros::dec;

    fn cfg() -> FeeGuardConfig {
        FeeGuardConfig {
            fee_bps: dec!(4),
            estimated_slippage_bps: dec!(2),
            max_spread_bps: dec!(10),
            min_net_reward_risk: dec!(1.2),
            min_net_reward_bps: dec!(10),
            require_bracket_for_entry: true,
        }
    }

    fn req(side: OrderSide) -> OrderRequest {
        OrderRequest {
            symbol: "BTCUSDT".into(),
            order_type: OrderType::Market,
            side,
            qty: dec!(1),
            ..Default::default()
        }
    }

    #[test]
    fn long_entry_passes_when_net_edge_is_large_enough() {
        let decision = evaluate_entry(
            &req(OrderSide::Buy),
            Some(Bracket {
                stop_price: dec!(99),
                take_profit_price: dec!(103),
            }),
            Some(Quote {
                bid: dec!(99.98),
                ask: dec!(100.02),
            }),
            &cfg(),
        )
        .unwrap();

        assert!(decision.report.net_reward > Decimal::ZERO);
        assert!(decision.report.net_reward_risk >= dec!(1.2));
    }

    #[test]
    fn short_entry_passes_when_net_edge_is_large_enough() {
        let decision = evaluate_entry(
            &req(OrderSide::Sell),
            Some(Bracket {
                stop_price: dec!(101),
                take_profit_price: dec!(97),
            }),
            Some(Quote {
                bid: dec!(99.98),
                ask: dec!(100.02),
            }),
            &cfg(),
        )
        .unwrap();

        assert!(decision.report.net_reward > Decimal::ZERO);
        assert!(decision.report.net_reward_risk >= dec!(1.2));
    }

    #[test]
    fn rejects_missing_bracket() {
        let err = evaluate_entry(
            &req(OrderSide::Buy),
            None,
            Some(Quote {
                bid: dec!(99.98),
                ask: dec!(100.02),
            }),
            &cfg(),
        )
        .unwrap_err();

        assert_eq!(err, FeeGuardReject::MissingBracket);
    }

    #[test]
    fn rejects_wide_spread() {
        let err = evaluate_entry(
            &req(OrderSide::Buy),
            Some(Bracket {
                stop_price: dec!(99),
                take_profit_price: dec!(103),
            }),
            Some(Quote {
                bid: dec!(99),
                ask: dec!(101),
            }),
            &cfg(),
        )
        .unwrap_err();

        assert_eq!(err, FeeGuardReject::SpreadTooWide);
    }

    #[test]
    fn rejects_low_net_reward() {
        let err = evaluate_entry(
            &req(OrderSide::Buy),
            Some(Bracket {
                stop_price: dec!(99),
                take_profit_price: dec!(100.12),
            }),
            Some(Quote {
                bid: dec!(99.98),
                ask: dec!(100.02),
            }),
            &cfg(),
        )
        .unwrap_err();

        assert_eq!(err, FeeGuardReject::RewardBpsTooLow);
    }

    #[test]
    fn extracts_matching_bracket_from_command_batch() {
        let cmds = vec![
            StrategyCommand::SetStopLoss {
                side: OrderSide::Sell,
                qty: dec!(1),
                stop_price: dec!(99),
            },
            StrategyCommand::SetTakeProfit {
                side: OrderSide::Sell,
                qty: dec!(1),
                stop_price: dec!(103),
            },
        ];
        let bracket = bracket_for(OrderSide::Buy, dec!(1), &cmds).unwrap();
        assert_eq!(bracket.stop_price, dec!(99));
        assert_eq!(bracket.take_profit_price, dec!(103));
    }
}
