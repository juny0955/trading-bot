use crate::domain::order::{Fill, Order, OrderSide, OrderStatus};
use crate::domain::position::PositionAction;
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// 체결 이벤트를 알림 메시지 문자열로 변환한다.
///
/// # Examples
///
/// ```ignore
/// let msg = format_fill(&fill);
/// assert!(msg.contains("✅"));
/// assert!(msg.contains(&fill.symbol));
/// ```
pub fn format_fill(fill: &Fill) -> String {
    format!(
        "✅ 체결\n{} · {}\n수량: {} @ {} USDT\n수수료: {} {}",
        fill.symbol,
        side_label(fill.side),
        fill.qty,
        fill.price,
        fill.fee,
        fill.fee_asset,
    )
}

/// 포지션 액션 컨텍스트를 포함한 체결 알림 메시지를 생성한다.
///
/// 신규 진입·추가 진입·청산을 구분하며, 청산 시 수익(USDT)과 수익률(%)을 포함한다.
///
/// # Examples
///
/// ```ignore
/// let msg = format_fill_with_action(&fill, &PositionAction::Opened);
/// assert!(msg.starts_with("📈"));
/// ```
pub fn format_fill_with_action(fill: &Fill, action: &PositionAction) -> String {
    match action {
        PositionAction::Opened => format!(
            "📈 진입\n{} · {}\n수량: {} @ {} USDT\n수수료: {} {}",
            fill.symbol,
            side_label(fill.side),
            fill.qty,
            fill.price,
            fill.fee,
            fill.fee_asset,
        ),
        PositionAction::Increased {
            new_avg_entry_price,
        } => format!(
            "📈 추가 진입\n{} · {}\n수량: {} @ {} USDT (평균: {} USDT)\n수수료: {} {}",
            fill.symbol,
            side_label(fill.side),
            fill.qty,
            fill.price,
            new_avg_entry_price,
            fill.fee,
            fill.fee_asset,
        ),
        PositionAction::PartialClose {
            closed_qty,
            this_pnl,
            pnl_pct,
        } => format!(
            "📉 부분 청산\n{} · {}\n수량: {} @ {} USDT\n수익: {} USDT (수익률: {})\n수수료: {} {}",
            fill.symbol,
            closing_side_label(fill.side),
            closed_qty,
            fill.price,
            format_pnl(*this_pnl),
            format_pct(*pnl_pct),
            fill.fee,
            fill.fee_asset,
        ),
        PositionAction::FullClose { this_pnl, pnl_pct } => format!(
            "🏁 청산\n{} · {}\n수량: {} @ {} USDT\n수익: {} USDT (수익률: {})\n수수료: {} {}",
            fill.symbol,
            closing_side_label(fill.side),
            fill.qty,
            fill.price,
            format_pnl(*this_pnl),
            format_pct(*pnl_pct),
            fill.fee,
            fill.fee_asset,
        ),
        PositionAction::Flipped {
            closed_qty,
            this_pnl,
            pnl_pct,
        } => {
            let open_qty = flipped_open_qty(fill.qty, *closed_qty);
            format!(
                "🔄 방향 전환\n{}  {}→{}\n청산: {} @ {} USDT (수익: {} USDT / {})\n신규: {} {} @ {} USDT\n수수료: {} {}",
                fill.symbol,
                closing_side_label(fill.side),
                side_label(fill.side),
                closed_qty,
                fill.price,
                format_pnl(*this_pnl),
                format_pct(*pnl_pct),
                open_qty,
                side_label(fill.side),
                fill.price,
                fill.fee,
                fill.fee_asset,
            )
        }
    }
}

/// 취소·거부·만료 주문을 알림 메시지 문자열로 변환한다.
///
/// # Examples
///
/// ```ignore
/// let msg = format_order_status(&cancelled_order);
/// assert!(msg.contains("❌"));
/// ```
pub fn format_order_status(order: &Order) -> String {
    let (icon, label) = status_icon_and_label(order.status);
    format!(
        "{} {} — {} {} {}",
        icon,
        label,
        order.symbol,
        side_label(order.side),
        order.qty,
    )
}

/// 하루치 체결 목록을 일별 요약 메시지로 변환한다.
///
/// # Examples
///
/// ```ignore
/// let msg = format_daily_summary(&fills, Utc::now());
/// assert!(msg.starts_with("📊"));
/// ```
pub fn format_daily_summary(fills: &[Fill], date: DateTime<Utc>) -> String {
    let date_label = date.format("%Y-%m-%d");
    if fills.is_empty() {
        return format!("📊 일일 리포트 {date_label} — 체결 없음");
    }

    let (buy_count, total_volume, total_fee) = summarize_fills(fills);
    let count = fills.len();
    let sell_count = count - buy_count;

    format!(
        "📊 일일 리포트 {date_label}\n거래: {count}건 (매수 {buy_count} / 매도 {sell_count})\n거래량: {total_volume:.2} USDT\n수수료: {total_fee:.4} USDT",
    )
}

// 체결 목록을 단일 패스로 집계한다: (매수 건수, 총 거래대금, 총 수수료)
fn summarize_fills(fills: &[Fill]) -> (usize, Decimal, Decimal) {
    fills.iter().fold(
        (0usize, Decimal::ZERO, Decimal::ZERO),
        |(buy, vol, fee), f| {
            let is_buy = (f.side == OrderSide::Buy) as usize;
            (buy + is_buy, vol + f.qty * f.price, fee + f.fee)
        },
    )
}

fn status_icon_and_label(status: OrderStatus) -> (&'static str, &'static str) {
    match status {
        OrderStatus::Cancelled => ("❌", "주문 취소"),
        OrderStatus::Rejected => ("⚠️", "주문 거부"),
        OrderStatus::Expired => ("⏱", "주문 만료"),
        _ => ("ℹ️", "주문 상태 변경"),
    }
}

fn side_label(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "LONG",
        OrderSide::Sell => "SHORT",
    }
}

// 청산 체결의 fill.side는 포지션 반대 방향이므로 원래 포지션 방향을 반환한다.
fn closing_side_label(fill_side: OrderSide) -> &'static str {
    match fill_side {
        OrderSide::Sell => "LONG",
        OrderSide::Buy => "SHORT",
    }
}

// Flipped 체결에서 신규 포지션 수량을 계산한다. fill_qty > closed_qty 임이 보장됨.
fn flipped_open_qty(fill_qty: Decimal, closed_qty: Decimal) -> Decimal {
    fill_qty - closed_qty
}

// 양수는 "+" 접두사를 붙이고 음수는 Decimal 자체의 "-" 부호를 사용한다.
fn format_pnl(pnl: Decimal) -> String {
    if pnl >= Decimal::ZERO {
        format!("+{:.2}", pnl)
    } else {
        format!("{:.2}", pnl)
    }
}

// 양수 수익률에 "+" 접두사를 붙여 손익 방향을 명확히 표시한다.
fn format_pct(pct: Decimal) -> String {
    if pct >= Decimal::ZERO {
        format!("+{:.2}%", pct)
    } else {
        format!("{:.2}%", pct)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;
    use uuid::Uuid;

    fn make_fill(side: OrderSide, qty: Decimal, price: Decimal, fee: Decimal) -> Fill {
        Fill {
            order_id: Uuid::nil(),
            strategy_instance_id: 1,
            symbol: "BTCUSDT".into(),
            side,
            qty,
            price,
            fee,
            fee_asset: "USDT".into(),
            filled_at: Utc::now(),
            exchange_trade_id: 1,
        }
    }

    #[test]
    fn format_fill_contains_key_fields() -> Result<()> {
        let fill = make_fill(OrderSide::Buy, dec!(0.5), dec!(42100), dec!(21.05));
        let msg = format_fill(&fill);
        assert!(msg.contains("BTCUSDT"));
        assert!(msg.contains("LONG"));
        assert!(msg.contains("0.5"));
        assert!(msg.contains("42100"));
        Ok(())
    }

    #[test]
    fn format_fill_sell_side_shows_short() -> Result<()> {
        let fill = make_fill(OrderSide::Sell, dec!(1), dec!(50000), dec!(0.5));
        let msg = format_fill(&fill);
        assert!(msg.contains("SHORT"));
        Ok(())
    }

    #[test]
    fn format_order_status_cancelled_shows_icon() -> Result<()> {
        use crate::domain::order::{Order, OrderRequest, OrderType};
        let req = OrderRequest {
            symbol: "ETHUSDT".into(),
            order_type: OrderType::Market,
            side: OrderSide::Sell,
            qty: dec!(2),
            strategy_instance_id: 1,
            ..Default::default()
        };
        let mut order = Order::new_from_request(req)?;
        order.status = OrderStatus::Cancelled;
        let msg = format_order_status(&order);
        assert!(msg.contains("❌"));
        assert!(msg.contains("ETHUSDT"));
        Ok(())
    }

    #[test]
    fn format_daily_summary_empty_returns_no_fills_label() -> Result<()> {
        let date = Utc.with_ymd_and_hms(2025, 5, 24, 0, 0, 0).unwrap();
        let msg = format_daily_summary(&[], date);
        assert!(msg.contains("체결 없음"));
        Ok(())
    }

    #[test]
    fn format_daily_summary_aggregates_buy_sell_counts() -> Result<()> {
        let date = Utc.with_ymd_and_hms(2025, 5, 24, 0, 0, 0).unwrap();
        let fills = vec![
            make_fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0.1)),
            make_fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0.1)),
        ];
        let msg = format_daily_summary(&fills, date);
        // 2건, 매수 1 / 매도 1 확인
        assert!(msg.contains("2건"));
        assert!(msg.contains("매수 1"));
        assert!(msg.contains("매도 1"));
        Ok(())
    }

    #[test]
    fn summarize_fills_single_pass_matches_manual() -> Result<()> {
        let fills = vec![
            make_fill(OrderSide::Buy, dec!(2), dec!(100), dec!(0.2)),
            make_fill(OrderSide::Buy, dec!(1), dec!(200), dec!(0.1)),
            make_fill(OrderSide::Sell, dec!(3), dec!(150), dec!(0.3)),
        ];
        let (buy, vol, fee) = summarize_fills(&fills);
        assert_eq!(buy, 2);
        assert_eq!(vol, dec!(200) + dec!(200) + dec!(450));
        assert_eq!(fee, dec!(0.6));
        Ok(())
    }

    #[test]
    fn format_opened_shows_entry_icon() -> Result<()> {
        let fill = make_fill(OrderSide::Buy, dec!(0.1), dec!(42000), dec!(0.42));
        let msg = format_fill_with_action(&fill, &PositionAction::Opened);
        assert!(msg.contains("📈"));
        assert!(msg.contains("진입"));
        assert!(msg.contains("LONG"));
        assert!(!msg.contains("추가")); // 추가 진입이 아님을 확인
        Ok(())
    }

    #[test]
    fn format_increased_shows_avg_price() -> Result<()> {
        let fill = make_fill(OrderSide::Buy, dec!(0.1), dec!(43000), dec!(0.43));
        let action = PositionAction::Increased {
            new_avg_entry_price: dec!(42500),
        };
        let msg = format_fill_with_action(&fill, &action);
        assert!(msg.contains("추가 진입"));
        assert!(msg.contains("42500")); // 평균 진입가 포함
        Ok(())
    }

    #[test]
    fn format_partial_close_shows_pnl() -> Result<()> {
        let fill = make_fill(OrderSide::Sell, dec!(0.05), dec!(44000), dec!(0.44));
        let action = PositionAction::PartialClose {
            closed_qty: dec!(0.05),
            this_pnl: dec!(100),
            pnl_pct: dec!(10),
        };
        let msg = format_fill_with_action(&fill, &action);
        assert!(msg.contains("부분 청산"));
        assert!(msg.contains("LONG")); // closing_side_label(Sell) = LONG
        assert!(msg.contains("+100.00"));
        assert!(msg.contains("+10.00%"));
        Ok(())
    }

    #[test]
    fn format_full_close_shows_pnl() -> Result<()> {
        let fill = make_fill(OrderSide::Sell, dec!(0.1), dec!(44000), dec!(0.44));
        let action = PositionAction::FullClose {
            this_pnl: dec!(200),
            pnl_pct: dec!(4.76),
        };
        let msg = format_fill_with_action(&fill, &action);
        assert!(msg.contains("🏁"));
        assert!(msg.contains("청산"));
        assert!(msg.contains("+200.00"));
        Ok(())
    }

    #[test]
    fn format_flipped_shows_direction_change() -> Result<()> {
        // LONG 0.1 포지션 보유 중, Sell 0.3으로 청산 + SHORT 0.2 오픈
        let fill = make_fill(OrderSide::Sell, dec!(0.3), dec!(44000), dec!(0.44));
        let action = PositionAction::Flipped {
            closed_qty: dec!(0.1),
            this_pnl: dec!(200),
            pnl_pct: dec!(4.76),
        };
        let msg = format_fill_with_action(&fill, &action);
        assert!(msg.contains("🔄"));
        assert!(msg.contains("방향 전환"));
        assert!(msg.contains("LONG→SHORT")); // closing(Sell)=LONG, side(Sell)=SHORT
        Ok(())
    }
}
