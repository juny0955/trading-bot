use crate::domain::order::{Fill, Order, OrderSide, OrderStatus};
use chrono::{DateTime, Utc};
use rust_decimal::Decimal;

/// 체결 이벤트를 텔레그램 메시지 문자열로 변환한다.
pub fn format_fill(fill: &Fill) -> String {
    let side = side_label(fill.side);
    format!(
        "✅ 체결\n{} · {}\n수량: {} @ {} USDT\n수수료: {} {}",
        fill.symbol, side, fill.qty, fill.price, fill.fee, fill.fee_asset,
    )
}

/// 취소·거부·만료 주문을 텔레그램 메시지 문자열로 변환한다.
pub fn format_order_status(order: &Order) -> String {
    let icon = match order.status {
        OrderStatus::Cancelled => "❌",
        OrderStatus::Rejected => "⚠️",
        OrderStatus::Expired => "⏱",
        _ => "ℹ️",
    };
    let status_label = match order.status {
        OrderStatus::Cancelled => "주문 취소",
        OrderStatus::Rejected => "주문 거부",
        OrderStatus::Expired => "주문 만료",
        _ => "주문 상태 변경",
    };
    format!(
        "{} {} — {} {} {}",
        icon,
        status_label,
        order.symbol,
        side_label(order.side),
        order.qty,
    )
}

/// 하루치 체결 목록을 일별 요약 메시지로 변환한다.
///
/// 체결이 없으면 빈 `String`을 반환한다.
pub fn format_daily_summary(fills: &[Fill], date: DateTime<Utc>) -> String {
    if fills.is_empty() {
        return format!("📊 일일 리포트 {} — 체결 없음", date.format("%Y-%m-%d"));
    }

    let count = fills.len();
    let buy_count = fills.iter().filter(|f| f.side == OrderSide::Buy).count();
    let sell_count = count - buy_count;
    let total_volume: Decimal = fills.iter().map(|f| f.qty * f.price).sum();
    let total_fee: Decimal = fills.iter().map(|f| f.fee).sum();

    format!(
        "📊 일일 리포트 {}\n거래: {}건 (매수 {} / 매도 {})\n거래량: {:.2} USDT\n수수료: {:.4} USDT",
        date.format("%Y-%m-%d"),
        count,
        buy_count,
        sell_count,
        total_volume,
        total_fee,
    )
}

fn side_label(side: OrderSide) -> &'static str {
    match side {
        OrderSide::Buy => "LONG",
        OrderSide::Sell => "SHORT",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
    fn format_fill_contains_key_fields() {
        let fill = make_fill(OrderSide::Buy, dec!(0.5), dec!(42100), dec!(21.05));
        let msg = format_fill(&fill);
        assert!(msg.contains("BTCUSDT"));
        assert!(msg.contains("LONG"));
        assert!(msg.contains("0.5"));
        assert!(msg.contains("42100"));
    }

    #[test]
    fn format_daily_summary_empty_fills() {
        let date = Utc.with_ymd_and_hms(2025, 5, 24, 0, 0, 0).unwrap();
        let msg = format_daily_summary(&[], date);
        assert!(msg.contains("체결 없음"));
    }

    #[test]
    fn format_daily_summary_aggregates_correctly() {
        let date = Utc.with_ymd_and_hms(2025, 5, 24, 0, 0, 0).unwrap();
        let fills = vec![
            make_fill(OrderSide::Buy, dec!(1), dec!(100), dec!(0.1)),
            make_fill(OrderSide::Sell, dec!(1), dec!(110), dec!(0.1)),
        ];
        let msg = format_daily_summary(&fills, date);
        // 2건, 매수 1 / 매도 1
        assert!(msg.contains("2건"));
        assert!(msg.contains("매수 1"));
        assert!(msg.contains("매도 1"));
    }
}
