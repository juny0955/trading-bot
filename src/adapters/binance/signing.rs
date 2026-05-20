use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;

pub(crate) fn sign(secret: &str, payload: &str) -> String {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("HMAC 초기화 오류: secret 문제");
    mac.update(payload.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

pub(crate) fn timestamp_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("시스템 시간 UNIX epoch 이전")
        .as_millis() as u64
}
