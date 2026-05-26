use rust_decimal::Decimal;

/// 값이 양수(`> 0`)인지 검사합니다. 0 이하이면 오류 메시지 문자열을 반환합니다.
///
/// 도메인별로 다른 에러 타입에 `.map_err(ErrVariant)`로 매핑하는 공통 검증 로직입니다.
pub(crate) fn check_positive(name: &str, value: Decimal) -> Result<(), String> {
    if value <= Decimal::ZERO {
        Err(format!("{name} must be positive"))
    } else {
        Ok(())
    }
}

/// 값이 음수가 아닌지(`>= 0`) 검사합니다. 음수이면 오류 메시지 문자열을 반환합니다.
pub(crate) fn check_non_negative(name: &str, value: Decimal) -> Result<(), String> {
    if value.is_sign_negative() {
        Err(format!("{name} must not be negative"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn check_positive_rejects_zero_and_negative() {
        assert!(check_positive("qty", dec!(0)).is_err());
        assert!(check_positive("qty", dec!(-1)).is_err());
        assert!(check_positive("qty", dec!(0.001)).is_ok());
    }

    #[test]
    fn check_non_negative_rejects_negative_only() {
        assert!(check_non_negative("fee", dec!(-0.1)).is_err());
        assert!(check_non_negative("fee", dec!(0)).is_ok());
        assert!(check_non_negative("fee", dec!(1)).is_ok());
    }
}
