//! 轻量时间工具：只解析 ISO 8601（UTC 的 Z 或带时区偏移），避免引入额外依赖。

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

fn parse_digits(bytes: &[u8], start: usize, len: usize) -> Option<i64> {
    if start + len > bytes.len() {
        return None;
    }
    let mut v = 0i64;
    for &b in &bytes[start..start + len] {
        if !b.is_ascii_digit() {
            return None;
        }
        v = v * 10 + (b - b'0') as i64;
    }
    Some(v)
}

/// 解析 `2026-11-21T13:58:35.413Z` 这类时间戳，返回 Unix 毫秒。
pub fn parse_iso_ms(s: &str) -> Option<i64> {
    let b = s.trim().as_bytes();
    if b.len() < 19 {
        return None;
    }
    let y = parse_digits(b, 0, 4)?;
    if b[4] != b'-' {
        return None;
    }
    let mo = parse_digits(b, 5, 2)?;
    if b[7] != b'-' {
        return None;
    }
    let d = parse_digits(b, 8, 2)?;
    if b[10] != b'T' && b[10] != b' ' {
        return None;
    }
    let h = parse_digits(b, 11, 2)?;
    if b[13] != b':' {
        return None;
    }
    let mi = parse_digits(b, 14, 2)?;
    if b[16] != b':' {
        return None;
    }
    let se = parse_digits(b, 17, 2)?;
    let mut idx = 19;
    let mut millis = 0i64;
    if idx < b.len() && b[idx] == b'.' {
        let start = idx + 1;
        let mut end = start;
        while end < b.len() && b[end].is_ascii_digit() {
            end += 1;
        }
        let digits = &b[start..end];
        let take = digits.len().min(3);
        let mut v = 0i64;
        for &c in &digits[..take] {
            v = v * 10 + (c - b'0') as i64;
        }
        for _ in take..3 {
            v *= 10;
        }
        millis = v;
        idx = end;
    }
    let mut offset_min = 0i64;
    if idx < b.len() {
        match b[idx] {
            b'Z' | b'z' => {}
            b'+' | b'-' => {
                let sign = if b[idx] == b'-' { -1 } else { 1 };
                let oh = parse_digits(b, idx + 1, 2)?;
                let om = if idx + 4 <= b.len() && b[idx + 3] == b':' {
                    parse_digits(b, idx + 4, 2)?
                } else if idx + 4 <= b.len() {
                    parse_digits(b, idx + 3, 2)?
                } else {
                    0
                };
                offset_min = sign * (oh * 60 + om);
            }
            _ => return None,
        }
    }
    let days = days_from_civil(y, mo, d);
    Some(days * 86_400_000 + h * 3_600_000 + mi * 60_000 + se * 1000 + millis - offset_min * 60_000)
}

/// 由 Unix 毫秒生成 `YYYY-MM-DDTHH:MM:SS.sssZ`
pub fn iso_from_ms(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let millis = ms.rem_euclid(1000);
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        y,
        m,
        d,
        rem / 3600,
        (rem % 3600) / 60,
        rem % 60,
        millis
    )
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
