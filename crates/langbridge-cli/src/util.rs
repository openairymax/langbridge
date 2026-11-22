// 小型通用工具

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// 生成 RFC 4122 形状的请求 ID（不依赖 uuid crate）
pub fn new_request_id() -> String {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0);
    let pid = std::process::id() as u64;
    let mut a = nanos;
    let mut b = nanos.rotate_left(17) ^ pid ^ n.wrapping_mul(0x9E37_79B9_7F4A_7C15);
    if b == 0 {
        b = 0x1234_5678_9ABC_DEF0;
    }
    let mut hasher = DefaultHasher::new();
    (nanos, pid, n).hash(&mut hasher);
    let c = hasher.finish();
    let d = a.wrapping_mul(6364136223846793005).wrapping_add(c);
    a ^= d;
    let bytes = [
        (a >> 56) as u8,
        (a >> 48) as u8,
        (a >> 40) as u8,
        (a >> 32) as u8,
        (b >> 56) as u8,
        (b >> 48) as u8,
        (b >> 40) as u8,
        (b >> 32) as u8,
        (c >> 56) as u8,
        (c >> 48) as u8,
        (c >> 40) as u8,
        (c >> 32) as u8,
        (d >> 56) as u8,
        (d >> 48) as u8,
        (d >> 40) as u8,
        (d >> 32) as u8,
    ];
    let mut bytes = bytes;
    bytes[6] = (bytes[6] & 0x0F) | 0x40;
    bytes[8] = (bytes[8] & 0x3F) | 0x80;
    let hex: Vec<String> = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    format!(
        "{}-{}-{}-{}-{}",
        hex[0..4].concat(),
        hex[4..6].concat(),
        hex[6..8].concat(),
        hex[8..10].concat(),
        hex[10..16].concat()
    )
}

pub fn iso_from_ms(ms: i64) -> String {
    langbridge_core::time::iso_from_ms(ms)
}

pub fn error_code_str(code: langbridge_core::ErrorCode) -> &'static str {
    use langbridge_core::ErrorCode::*;
    match code {
        EmptyInput => "empty_input",
        UnknownModel => "unknown_model",
        NoEvidence => "no_evidence",
        UsageMissing => "usage_missing",
        LowFit => "low_fit",
        SampleTooSmall => "sample_too_small",
        Outlier => "outlier",
        InvalidProfile => "invalid_profile",
        StorageFull => "storage_full",
        StorageUnavailable => "storage_unavailable",
        CorsBlocked => "cors_blocked",
        Unauthorized => "unauthorized",
        RateLimited => "rate_limited",
        Timeout => "timeout",
        ExpiredProfile => "expired_profile",
        BadBaseUrl => "bad_base_url",
        BadModel => "bad_model",
        BadRequest => "bad_request",
        BadResponse => "bad_response",
        NetworkError => "network_error",
        ServerError => "server_error",
        Cancelled => "cancelled",
    }
}

pub fn exit_code_for_error(code: langbridge_core::ErrorCode) -> i32 {
    use langbridge_core::ErrorCode::*;
    match code {
        EmptyInput | UnknownModel | BadBaseUrl | BadModel | BadRequest => 1,
        InvalidProfile => 4,
        Unauthorized | RateLimited | Timeout | CorsBlocked | BadResponse | ServerError
        | NetworkError => 3,
        _ => 2,
    }
}
