//! 内置证据：离线校准结果与 API 探测的纯逻辑。

use std::sync::OnceLock;

use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use url::Url;

use crate::calibrate::{calibration_usable, compute_calibration, CalibrationMeta};
use crate::samples::{build_sample, SCALES};
use crate::types::{
    CalibrationResult, EngineError, ErrorCode, Lang, Observation, ProfileSource, TokenizerMeta,
    Warning,
};

pub const BUILTIN_EVIDENCE_VERSION: u32 = 2;

#[derive(Debug, Clone, Deserialize)]
pub struct EvidenceSample {
    pub sample_id: String,
    pub lang: Lang,
    pub n: f64,
    pub sha256: String,
    pub prompt_tokens: f64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvidenceDataset {
    pub model_id: String,
    pub tokenizer: TokenizerMeta,
    pub observations: Vec<Observation>,
    pub samples: Vec<EvidenceSample>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct EvidenceFile {
    pub version: u32,
    pub generated_at: String,
    pub note: String,
    pub sample_set_sha256: String,
    pub datasets: Vec<EvidenceDataset>,
}

const EVIDENCE_JSON: &str = include_str!("../data/builtin-evidence.json");

pub fn evidence() -> &'static EvidenceFile {
    static CELL: OnceLock<EvidenceFile> = OnceLock::new();
    CELL.get_or_init(|| serde_json::from_str(EVIDENCE_JSON).expect("内置证据 JSON 应可解析"))
}

pub fn evidence_version_ok() -> bool {
    evidence().version == BUILTIN_EVIDENCE_VERSION
}

#[derive(Debug, Clone)]
pub struct EvidenceMeta {
    pub version: u32,
    pub generated_at: String,
    pub note: String,
    pub sample_set_sha256: String,
    pub tokenizers: Vec<(String, TokenizerMeta)>,
}

pub fn evidence_meta() -> EvidenceMeta {
    let file = evidence();
    EvidenceMeta {
        version: file.version,
        generated_at: file.generated_at.clone(),
        note: file.note.clone(),
        sample_set_sha256: file.sample_set_sha256.clone(),
        tokenizers: file
            .datasets
            .iter()
            .map(|d| (d.model_id.clone(), d.tokenizer.clone()))
            .collect(),
    }
}

pub fn list_evidence_models() -> Vec<String> {
    evidence()
        .datasets
        .iter()
        .map(|d| d.model_id.clone())
        .collect()
}

pub fn has_evidence(model_id: &str) -> bool {
    evidence().datasets.iter().any(|d| d.model_id == model_id)
}

/// 由内置证据直接得到校准结果，不访问网络。
pub fn calibrate_from_evidence(model_id: &str) -> Result<CalibrationResult, EngineError> {
    let dataset = evidence()
        .datasets
        .iter()
        .find(|d| d.model_id == model_id)
        .ok_or_else(|| {
            EngineError::new(
                ErrorCode::NoEvidence,
                format!("模型 {} 没有内置证据，请使用 API 探测或手动录入", model_id),
            )
        })?;
    let cal = compute_calibration(
        &dataset.observations,
        &CalibrationMeta {
            method: ProfileSource::Builtin,
            tokenizer: dataset.tokenizer.clone(),
            measured_at: evidence().generated_at.clone(),
            generation_ratio: None,
        },
    )?;
    if !calibration_usable(&cal) {
        let msg = if cal.warnings.contains(&Warning::SampleTooSmall) {
            "内置证据样本过小"
        } else {
            "内置证据未通过质量闸门"
        };
        let code = if cal.warnings.contains(&Warning::SampleTooSmall) {
            ErrorCode::SampleTooSmall
        } else {
            ErrorCode::LowFit
        };
        return Err(EngineError::new(code, msg));
    }
    Ok(cal)
}

fn scheme_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"^([a-z][a-z0-9+.-]*):/+").unwrap())
}

fn host_of(raw: &str) -> String {
    let authority = raw
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("");
    if let Some(rest) = authority.strip_prefix('[') {
        return rest.split(']').next().unwrap_or("").to_ascii_lowercase();
    }
    authority
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_local_upstream_host(host: &str) -> bool {
    if host == "localhost" || host == "127.0.0.1" || host == "::1" {
        return true;
    }
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() != 4
        || parts
            .iter()
            .any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit()))
    {
        return false;
    }
    let nums: Vec<u32> = parts.iter().filter_map(|p| p.parse().ok()).collect();
    if nums.len() != 4 || nums.iter().any(|n| *n > 255) {
        return false;
    }
    let (a, b) = (nums[0], nums[1]);
    a == 10 || a == 127 || (a == 172 && (16..=31).contains(&b)) || (a == 192 && b == 168)
}

/// 归一化 Base URL：补协议与 /v1，识别 ModelArts 与 Azure。
pub fn normalize_base(base_url: &str) -> String {
    let raw = base_url.trim();
    if raw.is_empty() {
        return String::new();
    }
    let with_scheme = if let Some(caps) = scheme_re().captures(raw) {
        let whole = caps.get(0).map(|m| m.end()).unwrap_or(0);
        format!("{}://{}", &caps[1], &raw[whole..])
    } else {
        let scheme = if is_local_upstream_host(&host_of(raw)) {
            "http"
        } else {
            "https"
        };
        format!("{}://{}", scheme, raw)
    };
    let Ok(mut url) = Url::parse(&with_scheme) else {
        return String::new();
    };
    if url.scheme() != "http" && url.scheme() != "https" {
        return String::new();
    }
    url.set_fragment(None);
    let path = url.path().trim_end_matches('/').to_string();
    if path.ends_with("/chat/completions") {
        url.set_path(&path);
        return url.to_string();
    }
    if url
        .host_str()
        .map(|h| h.eq_ignore_ascii_case("api.modelarts-maas.com"))
        .unwrap_or(false)
        && (path.is_empty() || path == "/v1" || path == "/openai")
    {
        url.set_path("/openai/v1/chat/completions");
        return url.to_string();
    }
    let prefix = if path.is_empty() {
        "/v1"
    } else {
        path.as_str()
    };
    url.set_path(&format!("{}/chat/completions", prefix));
    url.to_string()
}

fn is_azure_endpoint(url: &str) -> bool {
    Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
        .map(|h| h.ends_with(".openai.azure.com"))
        .unwrap_or(false)
}

#[derive(Debug, Clone)]
pub struct RequestTarget {
    pub url: String,
    pub headers: Vec<(String, String)>,
}

pub fn request_target(base_url: &str, api_key: &str) -> Result<RequestTarget, EngineError> {
    let url = normalize_base(base_url);
    if url.is_empty() {
        return Err(EngineError::new(
            ErrorCode::BadBaseUrl,
            "Base URL 不是合法的 http(s) 地址",
        ));
    }
    let azure = is_azure_endpoint(&url);
    if azure {
        let has_version = Url::parse(&url)
            .ok()
            .map(|u| u.query_pairs().any(|(k, _)| k == "api-version"))
            .unwrap_or(false);
        if !has_version {
            return Err(EngineError::new(
                ErrorCode::BadBaseUrl,
                "Azure OpenAI 需要 api-version：请填完整地址，例如 https://<资源>.openai.azure.com/openai/deployments/<部署名>/chat/completions?api-version=2024-10-21",
            ));
        }
    }
    let mut headers = vec![
        ("Content-Type".to_string(), "application/json".to_string()),
        ("Accept".to_string(), "application/json".to_string()),
    ];
    let key = api_key.trim();
    if !key.is_empty() {
        if azure {
            headers.push(("api-key".to_string(), key.to_string()));
        } else {
            headers.push(("Authorization".to_string(), format!("Bearer {}", key)));
        }
    }
    Ok(RequestTarget { url, headers })
}

pub fn classify_http(status: u16, server_message: &str) -> EngineError {
    let detail = if server_message.is_empty() {
        String::new()
    } else {
        format!("：{}", server_message)
    };
    match status {
        401 | 403 => EngineError::new(
            ErrorCode::Unauthorized,
            format!("Key 无效或没有该模型的权限（HTTP {}）{}", status, detail),
        ),
        404 => {
            let lower = server_message.to_ascii_lowercase();
            if lower.contains("model") || lower.contains("deployment") {
                EngineError::new(
                    ErrorCode::BadModel,
                    format!("服务端找不到这个模型或部署（HTTP 404）{}", detail),
                )
            } else {
                EngineError::new(
                    ErrorCode::BadBaseUrl,
                    format!(
                        "接口路径不存在，检查 Base URL（多数服务商需要 /v1）（HTTP 404）{}",
                        detail
                    ),
                )
            }
        }
        400 | 422 => EngineError::new(
            ErrorCode::BadRequest,
            format!("请求被拒绝（HTTP {}）{}", status, detail),
        ),
        408 => EngineError::new(
            ErrorCode::Timeout,
            format!("服务端等待超时（HTTP 408）{}", detail),
        ),
        429 => EngineError::new(
            ErrorCode::RateLimited,
            format!("触发限流（HTTP 429）{}", detail),
        ),
        s if s >= 500 => EngineError::new(
            ErrorCode::ServerError,
            format!("服务端错误（HTTP {}）{}", s, detail),
        ),
        s => EngineError::new(ErrorCode::BadRequest, format!("HTTP {}{}", s, detail)),
    }
}

pub fn is_retryable(code: ErrorCode) -> bool {
    matches!(
        code,
        ErrorCode::RateLimited
            | ErrorCode::ServerError
            | ErrorCode::NetworkError
            | ErrorCode::Timeout
    )
}

#[derive(Debug, Clone, Copy)]
pub struct PayloadCaps {
    pub use_completion_tokens: bool,
    pub use_temperature: bool,
    pub use_stream: bool,
}

impl Default for PayloadCaps {
    fn default() -> Self {
        Self {
            use_completion_tokens: false,
            use_temperature: true,
            use_stream: true,
        }
    }
}

const UNSUPPORTED_PARAM: &str = r"(?:unsupported|not\s+support(?:ed)?|unknown|unrecognized|unexpected|deprecated|invalid|not\s+allowed)";

fn rejected_param(text: &str) -> Option<&'static str> {
    let unsupported = Regex::new(UNSUPPORTED_PARAM).expect("参数正则应可编译");
    if !unsupported.is_match(text) {
        return None;
    }
    for param in [
        "max_completion_tokens",
        "max_tokens",
        "temperature",
        "stream",
    ] {
        let before = Regex::new(&format!(r"{}[^.;]{{0,40}}{}", UNSUPPORTED_PARAM, param)).unwrap();
        let after = Regex::new(&format!(r"{}{}[^.;]{{0,40}}", param, UNSUPPORTED_PARAM)).unwrap();
        if before.is_match(text) || after.is_match(text) {
            return Some(param);
        }
    }
    None
}

/// 根据错误信息关闭被拒绝的参数，每次只调整一项。
pub fn adapt_caps(message: &str, caps: &mut PayloadCaps) -> bool {
    let lower = message.to_ascii_lowercase();
    let Some(param) = rejected_param(&lower) else {
        return false;
    };
    match param {
        "max_completion_tokens" => {
            if !caps.use_completion_tokens {
                return false;
            }
            caps.use_completion_tokens = false;
            true
        }
        "max_tokens" => {
            if caps.use_completion_tokens {
                return false;
            }
            caps.use_completion_tokens = true;
            true
        }
        "temperature" => {
            if !caps.use_temperature {
                return false;
            }
            caps.use_temperature = false;
            true
        }
        _ => {
            if !caps.use_stream {
                return false;
            }
            caps.use_stream = false;
            true
        }
    }
}

pub fn build_payload(model_id: &str, text: &str, caps: &PayloadCaps) -> Value {
    let mut payload = json!({
        "model": model_id,
        "messages": [{ "role": "user", "content": text }],
    });
    let obj = payload.as_object_mut().unwrap();
    if caps.use_stream {
        obj.insert("stream".into(), json!(false));
    }
    if caps.use_completion_tokens {
        obj.insert("max_completion_tokens".into(), json!(1));
    } else {
        obj.insert("max_tokens".into(), json!(1));
    }
    if caps.use_temperature {
        obj.insert("temperature".into(), json!(0));
    }
    payload
}

fn extract_body_error(body: &Value) -> Option<String> {
    if let Some(err) = body.get("error") {
        if let Some(s) = err.as_str() {
            return Some(truncate(s, 200));
        }
        if let Some(m) = err.get("message").and_then(|m| m.as_str()) {
            return Some(truncate(m, 200));
        }
    }
    body.get("message")
        .and_then(|m| m.as_str())
        .map(|m| truncate(m, 200))
}

fn truncate(s: &str, max: usize) -> String {
    s.chars().take(max).collect()
}

pub fn classify_body_error(message: &str) -> ErrorCode {
    let text = message.to_ascii_lowercase();
    let has = |pat: &str| Regex::new(pat).unwrap().is_match(&text);
    if has(r"api key|unauthor|authenticat|invalid key|permission") {
        ErrorCode::Unauthorized
    } else if has(r"quota|insufficient|billing|balance|credit") {
        ErrorCode::RateLimited
    } else if has(r"model|deployment|not found|does not exist") {
        ErrorCode::BadModel
    } else if has(r"rate limit|too many requests") {
        ErrorCode::RateLimited
    } else {
        ErrorCode::BadResponse
    }
}

#[derive(Debug, Clone)]
pub struct ParsedUsage {
    pub prompt_tokens: Option<f64>,
    pub parsed: bool,
    pub error_message: Option<String>,
}

/// 兼容标准 JSON 与忽略 stream:false 的 SSE 网关，并识别 HTTP 200 响应体中的 error。
pub fn parse_usage(raw: &str) -> ParsedUsage {
    let trimmed = raw.trim().trim_start_matches('\u{feff}');
    let mut candidates: Vec<String> = vec![trimmed.to_string()];
    let mut data_lines: Vec<String> = trimmed
        .lines()
        .map(|l| l.trim())
        .filter(|l| l.starts_with("data:"))
        .map(|l| l[5..].trim().to_string())
        .filter(|l| !l.is_empty() && l != "[DONE]")
        .collect();
    data_lines.reverse();
    candidates.extend(data_lines);

    let mut parsed_any = false;
    for candidate in candidates {
        let Ok(body) = serde_json::from_str::<Value>(&candidate) else {
            continue;
        };
        parsed_any = true;
        if let Some(message) = extract_body_error(&body) {
            return ParsedUsage {
                prompt_tokens: None,
                parsed: true,
                error_message: Some(message),
            };
        }
        let usage = body.get("usage");
        let value = usage
            .and_then(|u| u.get("prompt_tokens"))
            .or_else(|| usage.and_then(|u| u.get("input_tokens")))
            .and_then(|v| v.as_f64());
        if let Some(v) = value {
            if v.is_finite() && v > 0.0 {
                return ParsedUsage {
                    prompt_tokens: Some(v),
                    parsed: true,
                    error_message: None,
                };
            }
        }
    }
    ParsedUsage {
        prompt_tokens: None,
        parsed: parsed_any,
        error_message: None,
    }
}

#[derive(Debug, Clone)]
pub struct ProbeStep {
    pub index: usize,
    pub total: usize,
    pub lang: Lang,
    pub n: f64,
}

pub fn probe_steps(base_n: f64) -> Vec<ProbeStep> {
    let total = SCALES.len() * 2;
    let mut steps = Vec::new();
    let mut index = 0;
    for scale in SCALES {
        for lang in [Lang::Zh, Lang::En] {
            index += 1;
            steps.push(ProbeStep {
                index,
                total,
                lang,
                n: base_n * scale as f64,
            });
        }
    }
    steps
}

pub fn step_sample(step: &ProbeStep, base_n: f64) -> String {
    build_sample(step.lang, (step.n / base_n) as u32)
}

pub fn calibration_from_probe(
    observations: &[Observation],
    model_id: &str,
    measured_at: String,
) -> Result<CalibrationResult, EngineError> {
    let cal = compute_calibration(
        observations,
        &CalibrationMeta {
            method: ProfileSource::ApiProbe,
            tokenizer: TokenizerMeta {
                name: format!("api:{}", model_id),
                version: "usage.prompt_tokens".to_string(),
                sha256: "n/a".to_string(),
                source: None,
                exact: None,
            },
            measured_at,
            generation_ratio: None,
        },
    )?;
    if !calibration_usable(&cal) {
        let msg = if cal.warnings.contains(&Warning::SampleTooSmall) {
            "API 校准未通过质量闸门：样本过小"
        } else {
            "API 校准未通过质量闸门：拟合质量或置信度不足"
        };
        let code = if cal.warnings.contains(&Warning::SampleTooSmall) {
            ErrorCode::SampleTooSmall
        } else {
            ErrorCode::LowFit
        };
        return Err(EngineError::new(code, msg));
    }
    Ok(cal)
}
