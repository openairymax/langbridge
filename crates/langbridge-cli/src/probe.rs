//! API 探测校准：对每个模型发 6 次请求，读取 usage 中的 prompt_tokens。

use std::time::Duration;

use langbridge_core::evidence::{
    adapt_caps, build_payload, calibration_from_probe, classify_body_error, classify_http,
    is_retryable, normalize_base, parse_usage, probe_steps, request_target, step_sample,
    PayloadCaps, RequestTarget,
};
use langbridge_core::{now_ms, CalibrationResult, EngineError, ErrorCode, Lang, Observation};
use serde_json::Value;

use crate::util::iso_from_ms;

const NETWORK_ERROR_MESSAGE: &str = "连不上：常见原因是跨域（CORS）被拦截，或网络不通";

fn read_server_message(raw: &str) -> String {
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(json) = serde_json::from_str::<Value>(raw) {
        let err = json.get("error");
        if let Some(Value::String(s)) = err {
            return take_chars(s, 200);
        }
        if let Some(obj) = err.and_then(Value::as_object) {
            if let Some(Value::String(m)) = obj.get("message") {
                return take_chars(m, 200);
            }
        }
        if let Some(Value::String(m)) = json.get("message") {
            return take_chars(m, 200);
        }
    }
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    take_chars(&collapsed, 200)
}

fn take_chars(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn retry_after_ms(headers: &[(String, String)]) -> Option<u64> {
    let header = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("retry-after"))
        .map(|(_, v)| v.trim())?;
    let seconds: f64 = header.parse().ok()?;
    if seconds.is_finite() && seconds > 0.0 {
        return Some(8_000.min((seconds * 1000.0).round() as u64));
    }
    None
}

fn backoff_ms(attempt: u32) -> u64 {
    let base = 600.0 * 2.5_f64.powi(attempt as i32);
    let jitter = (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.subsec_nanos())
        .unwrap_or(0) % 250) as f64;
    3_000.min((base + jitter).round() as u64)
}

fn is_timeout_text(msg: &str) -> bool {
    let lower = msg.to_ascii_lowercase();
    lower.contains("timed out") || lower.contains("timeout")
}

fn timeout_error(timeout_ms: u64) -> EngineError {
    EngineError::new(
        ErrorCode::Timeout,
        format!("请求超时（{} 秒无响应）", (timeout_ms as f64 / 1000.0).round() as i64),
    )
}

struct ProbeResponse {
    ok: bool,
    status: u16,
    headers: Vec<(String, String)>,
    body: String,
}

fn fetch_once(
    agent: &ureq::Agent,
    target: &RequestTarget,
    payload: &Value,
    timeout_ms: u64,
) -> Result<ProbeResponse, EngineError> {
    let mut req = agent.post(&target.url);
    for (k, v) in &target.headers {
        req = req.set(k, v);
    }
    let result = req.send_json(payload.clone());
    match result {
        Ok(resp) => {
            let status = resp.status();
            let headers = resp
                .headers_names()
                .into_iter()
                .filter_map(|name| resp.header(&name).map(|v| (name, v.to_string())))
                .collect();
            let body = resp.into_string().unwrap_or_default();
            Ok(ProbeResponse {
                ok: (200..300).contains(&status),
                status,
                headers,
                body,
            })
        }
        Err(ureq::Error::Status(status, resp)) => {
            let headers = resp
                .headers_names()
                .into_iter()
                .filter_map(|name| resp.header(&name).map(|v| (name, v.to_string())))
                .collect();
            let body = resp.into_string().unwrap_or_default();
            Ok(ProbeResponse {
                ok: false,
                status,
                headers,
                body,
            })
        }
        Err(ureq::Error::Transport(t)) => {
            let text = format!("{}", t);
            if is_timeout_text(&text) {
                Err(timeout_error(timeout_ms))
            } else {
                Err(EngineError::new(
                    ErrorCode::NetworkError,
                    NETWORK_ERROR_MESSAGE,
                ))
            }
        }
    }
}

pub struct ApiProbeOptions<'a> {
    pub base_url: &'a str,
    pub api_key: &'a str,
    pub model_id: &'a str,
    pub timeout_ms: Option<u64>,
    pub base_n: f64,
}

pub fn calibrate_via_api(opts: &ApiProbeOptions<'_>) -> Result<CalibrationResult, EngineError> {
    let base = normalize_base(opts.base_url);
    if base.is_empty() {
        return Err(EngineError::new(
            ErrorCode::BadBaseUrl,
            "请填写 Base URL，例如 https://api.deepseek.com/v1",
        ));
    }
    if opts.model_id.trim().is_empty() {
        return Err(EngineError::new(ErrorCode::BadModel, "请填写模型 ID"));
    }
    let target = request_target(opts.base_url, opts.api_key)?;
    let timeout_ms = opts.timeout_ms.unwrap_or(20_000);
    let retries: u32 = 1;
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms))
        .build();
    let steps = probe_steps(opts.base_n);
    let mut observations: Vec<Observation> = Vec::new();

    for step in &steps {
        let text = step_sample(step, opts.base_n);
        let mut caps = PayloadCaps::default();
        let mut attempt: u32 = 0;
        let mut adaptations: u32 = 3;
        loop {
            let payload = build_payload(opts.model_id, &text, &caps);
            let resp = match fetch_once(&agent, &target, &payload, timeout_ms) {
                Ok(r) => r,
                Err(err) => {
                    if is_retryable(err.code) && attempt < retries {
                        std::thread::sleep(Duration::from_millis(backoff_ms(attempt)));
                        attempt += 1;
                        continue;
                    }
                    return Err(err);
                }
            };

            if !resp.ok {
                let server_message = read_server_message(&resp.body);
                let error = classify_http(resp.status, &server_message);
                if adaptations > 0
                    && (resp.status == 400 || resp.status == 422)
                    && adapt_caps(&server_message, &mut caps)
                {
                    adaptations -= 1;
                    continue;
                }
                if is_retryable(error.code) && attempt < retries {
                    let delay = retry_after_ms(&resp.headers).unwrap_or_else(|| backoff_ms(attempt));
                    std::thread::sleep(Duration::from_millis(delay));
                    attempt += 1;
                    continue;
                }
                return Err(error);
            }

            let parsed = parse_usage(&resp.body);
            if let Some(message) = parsed.error_message {
                return Err(EngineError::new(
                    classify_body_error(&message),
                    format!("接口返回错误：{}", message),
                ));
            }
            if let Some(prompt_tokens) = parsed.prompt_tokens {
                observations.push(Observation {
                    sample_id: format!("{}-N{}", lang_code(step.lang), step.n as i64),
                    lang: step.lang,
                    n: step.n,
                    prompt_tokens,
                });
                break;
            }
            if !parsed.parsed {
                return Err(EngineError::new(
                    ErrorCode::BadResponse,
                    "返回的不是合法 JSON，可能 Base URL 指到了网页而不是接口",
                ));
            }
            return Err(EngineError::new(
                ErrorCode::UsageMissing,
                "响应里没有 usage.prompt_tokens：部分代理或流式接口不返回用量，可换官方 Base URL 或改用 CLI 校准",
            ));
        }
    }

    calibration_from_probe(&observations, opts.model_id, iso_from_ms(now_ms()))
}

fn lang_code(l: Lang) -> &'static str {
    match l {
        Lang::Zh => "zh",
        Lang::En => "en",
    }
}
