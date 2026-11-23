//! 本机 OpenAI 兼容代理：手写 HTTP/1.1，上游经 ureq 转发并做注入与后处理。

use std::collections::{HashMap, HashSet};
use std::io::{BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use langbridge_core::calibrate::should_recalibrate;
use langbridge_core::canonical::summarize_decision;
use langbridge_core::evidence::{
    calibrate_from_evidence, has_evidence, normalize_base,
};
use langbridge_core::guard::{inspect_only, postprocess, PostprocessOptions};
use langbridge_core::profile_store::{is_expired, profile_from_calibration};
use langbridge_core::profiles::{list_models, meta_for};
use langbridge_core::router::{route, RouteInput};
use langbridge_core::savings::estimate_r;
use langbridge_core::signals::extract_signals;
use langbridge_core::{now_ms, InjectionPlan, ModelProfile, Routing, SavingsResult, Signals};
use serde_json::Value;

use crate::format::js_num;
use crate::probe::{calibrate_via_api, ApiProbeOptions};
use crate::store::{read_profile, write_profile};
use crate::util::{iso_from_ms, new_request_id};

const MAX_BODY: usize = 8 * 1024 * 1024;
const MAX_STREAM_BUFFER: usize = 8 * 1024 * 1024;
const DEFAULT_UPSTREAM_TIMEOUT_MS: u64 = 120_000;
const DEFAULT_ALLOW_ORIGINS: &[&str] = &["https://langbridge.traesite.cn"];
const DEFAULT_ALLOW_HOSTS: &[&str] = &["localhost", "127.0.0.1", "::1"];
const ALLOW_HEADERS: &str = "authorization, content-type, api-key, openai-organization, openai-project, openai-beta, x-stainless-lang, x-stainless-package-version, x-stainless-os, x-stainless-arch, x-stainless-runtime, x-stainless-runtime-version";

#[derive(Clone)]
pub struct ProxyOptions {
    pub upstream: String,
    pub port: u16,
    pub host: String,
    pub api_key: Option<String>,
    pub model: String,
    pub profile: Option<ModelProfile>,
    pub quiet: bool,
    pub allow_origins: Vec<String>,
    pub allow_hosts: Vec<String>,
    pub upstream_timeout_ms: u64,
}

pub struct RunningProxy {
    pub url: String,
    pub handle: std::thread::JoinHandle<()>,
}

struct Shared {
    opts: ProxyOptions,
    ticks: Mutex<HashMap<String, u64>>,
    in_flight: Mutex<HashSet<String>>,
}

struct Request {
    method: String,
    url: String,
    headers: HashMap<String, String>,
    body: Vec<u8>,
}

enum ReadError {
    Closed,
    TooLarge,
    Bad(&'static str),
}

pub fn target_url(upstream: &str) -> Option<String> {
    let url = normalize_base(upstream);
    if url.is_empty() {
        None
    } else {
        Some(url)
    }
}

fn host_of(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(|s| s.to_string()))
        .unwrap_or_default()
}

fn normalize_origin(origin: &str) -> String {
    origin.trim().trim_end_matches('/').to_string()
}

fn is_allowed_origin(origin: &str, extra: &[String]) -> bool {
    let value = normalize_origin(origin);
    if value.is_empty() || value == "null" {
        return false;
    }
    if DEFAULT_ALLOW_ORIGINS.iter().any(|d| *d == value) {
        return true;
    }
    if extra.iter().any(|e| normalize_origin(e) == value) {
        return true;
    }
    let lower = value.to_ascii_lowercase();
    regex_is_local_origin(&lower)
}

fn regex_is_local_origin(value: &str) -> bool {
    let localhost = regex::Regex::new(r"^https?://localhost(?::\d+)?$").unwrap();
    let ipv4 = regex::Regex::new(r"^http://127\.0\.0\.1(?::\d+)?$").unwrap();
    let ipv6 = regex::Regex::new(r"^http://\[::1\](?::\d+)?$").unwrap();
    localhost.is_match(value) || ipv4.is_match(value) || ipv6.is_match(value)
}

fn host_name_of(value: Option<&str>) -> String {
    let Some(value) = value else {
        return String::new();
    };
    let raw = value.trim().to_ascii_lowercase();
    if raw.is_empty() {
        return String::new();
    }
    if raw.starts_with('[') {
        return match raw.find(']') {
            Some(end) if end > 1 => raw[1..end].to_string(),
            _ => String::new(),
        };
    }
    match (raw.find(':'), raw.rfind(':')) {
        (Some(first), Some(last)) if first == last => raw[..first].to_string(),
        _ => raw,
    }
}

fn is_allowed_host(header: Option<&str>, extra: &[String]) -> bool {
    let name = host_name_of(header);
    if name.is_empty() {
        return false;
    }
    let mut allowed: HashSet<String> = DEFAULT_ALLOW_HOSTS.iter().map(|s| s.to_string()).collect();
    for item in extra {
        allowed.insert(host_name_of(Some(item)));
    }
    allowed.contains(&name)
}

fn cors_headers(origin: &str) -> Vec<(String, String)> {
    vec![
        ("Access-Control-Allow-Origin".to_string(), origin.to_string()),
        ("Vary".to_string(), "Origin".to_string()),
        ("Access-Control-Allow-Headers".to_string(), ALLOW_HEADERS.to_string()),
        ("Access-Control-Allow-Methods".to_string(), "POST, GET, OPTIONS".to_string()),
        ("Access-Control-Max-Age".to_string(), "86400".to_string()),
    ]
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        413 => "Payload Too Large",
        502 => "Bad Gateway",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}

fn write_response(
    stream: &mut TcpStream,
    status: u16,
    extra_headers: &[(String, String)],
    body: &[u8],
) -> std::io::Result<()> {
    let mut head = format!(
        "HTTP/1.1 {} {}\r\nContent-Length: {}\r\nConnection: close\r\n",
        status,
        reason_phrase(status),
        body.len()
    );
    for (k, v) in extra_headers {
        head.push_str(&format!("{}: {}\r\n", k, v));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()
}

fn write_chunk(stream: &mut TcpStream, data: &[u8]) -> std::io::Result<()> {
    stream.write_all(format!("{:x}\r\n", data.len()).as_bytes())?;
    stream.write_all(data)?;
    stream.write_all(b"\r\n")
}

fn send_json(
    stream: &mut TcpStream,
    status: u16,
    cors: Option<&str>,
    payload: &Value,
) -> std::io::Result<()> {
    let mut headers = cors.map(cors_headers).unwrap_or_default();
    headers.push((
        "Content-Type".to_string(),
        "application/json; charset=utf-8".to_string(),
    ));
    let body = serde_json::to_vec(payload).unwrap_or_default();
    write_response(stream, status, &headers, &body)
}

fn send_error(stream: &mut TcpStream, status: u16, cors: Option<&str>, err_type: &str, message: &str) {
    let payload = serde_json::json!({ "error": { "message": message, "type": err_type } });
    let _ = send_json(stream, status, cors, &payload);
}

fn read_line(reader: &mut BufReader<TcpStream>) -> Option<String> {
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match reader.read(&mut byte) {
            Ok(0) => return None,
            Ok(_) => {
                if byte[0] == b'\n' {
                    break;
                }
                buf.push(byte[0]);
                if buf.len() > 64 * 1024 {
                    return None;
                }
            }
            Err(_) => return None,
        }
    }
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn read_exact_vec(reader: &mut BufReader<TcpStream>, len: usize) -> Result<Vec<u8>, ReadError> {
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf).map_err(|_| ReadError::Bad("read"))?;
    Ok(buf)
}

fn read_chunked(reader: &mut BufReader<TcpStream>) -> Result<Vec<u8>, ReadError> {
    let mut out = Vec::new();
    loop {
        let line = read_line(reader).ok_or(ReadError::Bad("chunk"))?;
        let size_text = line.split(';').next().unwrap_or("").trim();
        let size = usize::from_str_radix(size_text, 16).map_err(|_| ReadError::Bad("chunk"))?;
        if size == 0 {
            loop {
                let trailer = read_line(reader).ok_or(ReadError::Bad("chunk"))?;
                if trailer.is_empty() {
                    break;
                }
            }
            break;
        }
        if out.len() + size > MAX_BODY {
            return Err(ReadError::TooLarge);
        }
        let chunk = read_exact_vec(reader, size)?;
        out.extend_from_slice(&chunk);
        let _ = read_exact_vec(reader, 2)?;
    }
    Ok(out)
}

fn read_request(stream: &TcpStream) -> Result<Request, ReadError> {
    let mut reader = BufReader::new(stream.try_clone().map_err(|_| ReadError::Bad("clone"))?);
    let request_line = match read_line(&mut reader) {
        Some(l) => l,
        None => return Err(ReadError::Closed),
    };
    if request_line.is_empty() {
        return Err(ReadError::Closed);
    }
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or("").to_ascii_uppercase();
    let url = parts.next().unwrap_or("/").to_string();

    let mut headers: HashMap<String, String> = HashMap::new();
    loop {
        let line = read_line(&mut reader).ok_or(ReadError::Bad("header"))?;
        if line.is_empty() {
            break;
        }
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_ascii_lowercase();
            let value = line[colon + 1..].trim().to_string();
            headers.insert(key, value);
        }
    }

    let mut body = Vec::new();
    if let Some(te) = headers.get("transfer-encoding") {
        if te.to_ascii_lowercase().contains("chunked") {
            body = read_chunked(&mut reader)?;
        }
    } else if let Some(len) = headers.get("content-length") {
        let len: usize = len.trim().parse().map_err(|_| ReadError::Bad("length"))?;
        if len > MAX_BODY {
            return Err(ReadError::TooLarge);
        }
        if len > 0 {
            body = read_exact_vec(&mut reader, len)?;
        }
    }

    Ok(Request {
        method,
        url,
        headers,
        body,
    })
}

fn content_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(parts)) => parts
            .iter()
            .map(|part| {
                part.get("text")
                    .map(|t| match t {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

fn role_of(message: &Value) -> &str {
    message.get("role").and_then(Value::as_str).unwrap_or("")
}

fn last_user_text(messages: &[Value]) -> String {
    for message in messages.iter().rev() {
        if role_of(message) == "user" {
            return content_text(message.get("content"));
        }
    }
    String::new()
}

fn system_text(messages: &[Value]) -> String {
    messages
        .iter()
        .filter(|m| role_of(m) == "system")
        .map(|m| content_text(m.get("content")))
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn history_text(messages: &[Value]) -> String {
    let mut last_user: Option<usize> = None;
    for i in (0..messages.len()).rev() {
        if role_of(&messages[i]) == "user" {
            last_user = Some(i);
            break;
        }
    }
    messages
        .iter()
        .enumerate()
        .filter(|(i, _)| Some(*i) != last_user)
        .map(|(_, m)| content_text(m.get("content")))
        .collect::<Vec<_>>()
        .join("\n")
}

fn sse_text(sse: &str) -> String {
    let mut out = String::new();
    for line in sse.split(['\r', '\n']) {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("data:") else {
            continue;
        };
        let payload = rest.trim_start();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        if let Ok(parsed) = serde_json::from_str::<Value>(payload) {
            if let Some(piece) = parsed
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(Value::as_str)
            {
                out.push_str(piece);
            }
        }
    }
    out
}

fn with_injection(messages: &[Value], plan: &InjectionPlan) -> Vec<Value> {
    use langbridge_core::InjectionState;
    if plan.state == InjectionState::Disabled
        || plan.state == InjectionState::SkippedConflict
        || plan.system_prompt.is_empty()
    {
        return messages.to_vec();
    }
    if let Some(idx) = messages.iter().position(|m| role_of(m) == "system") {
        let current = content_text(messages[idx].get("content"));
        let merged = if current.is_empty() {
            plan.system_prompt.clone()
        } else {
            format!("{}\n\n{}", current, plan.system_prompt)
        };
        let mut out = messages.to_vec();
        if let Some(obj) = out[idx].as_object_mut() {
            obj.insert("content".to_string(), Value::String(merged));
        }
        out
    } else {
        let mut out = vec![serde_json::json!({ "role": "system", "content": plan.system_prompt })];
        out.extend_from_slice(messages);
        out
    }
}

fn probe_key(opts: &ProxyOptions) -> Option<String> {
    let raw = opts
        .api_key
        .clone()
        .or_else(|| std::env::var("LANGBRIDGE_API_KEY").ok())?;
    let trimmed = raw.trim();
    if trimmed.to_ascii_lowercase().starts_with("bearer ") {
        Some(trimmed[7..].trim().to_string())
    } else {
        Some(trimmed.to_string())
    }
}

fn probe_base_url(upstream: &str) -> String {
    let trimmed = upstream.trim_end_matches('/');
    trimmed
        .strip_suffix("/chat/completions")
        .unwrap_or(trimmed)
        .to_string()
}

fn has_valid_profile(model_id: &str) -> bool {
    matches!(read_profile(model_id), Ok(Some(p)) if !is_expired(&p, now_ms()))
}

fn resolve_profile(model_id: &str) -> Option<ModelProfile> {
    let stored = read_profile(model_id).ok().flatten();
    if let Some(ref p) = stored {
        if !is_expired(p, now_ms()) {
            return stored;
        }
    }
    if has_evidence(model_id) {
        if let Ok(cal) = calibrate_from_evidence(model_id) {
            return Some(profile_from_calibration(model_id, cal, None));
        }
    }
    stored
}

fn log_line(value: &Value, stderr: bool) {
    let text = serde_json::to_string(value).unwrap_or_default();
    if stderr {
        eprintln!("{}", text);
    } else {
        println!("{}", text);
    }
}

fn calibrate_model(shared: &Arc<Shared>, model_id: &str, reason: &str) {
    {
        let mut in_flight = shared.in_flight.lock().unwrap();
        if in_flight.contains(model_id) {
            return;
        }
        in_flight.insert(model_id.to_string());
    }
    let result: Result<(), String> = (|| {
        if has_evidence(model_id) {
            let cal = calibrate_from_evidence(model_id).map_err(|e| e.message)?;
            let profile = profile_from_calibration(model_id, cal, None);
            write_profile(&profile, None)
                .map_err(|e| e.message)?;
        } else {
            let Some(key) = probe_key(&shared.opts) else {
                if !shared.opts.quiet {
                    log_line(
                        &serde_json::json!({
                            "event": "calibration_skipped",
                            "model_id": model_id,
                            "reason": reason,
                            "detail": "无内置证据且未提供 API Key，保持未校准降级",
                        }),
                        false,
                    );
                }
                return Ok(());
            };
            let cal = calibrate_via_api(&ApiProbeOptions {
                base_url: &probe_base_url(&shared.opts.upstream),
                api_key: &key,
                model_id,
                timeout_ms: Some(shared.opts.upstream_timeout_ms),
                base_n: 120.0,
            })
            .map_err(|e| e.message)?;
            let profile = profile_from_calibration(model_id, cal, None);
            write_profile(&profile, None).map_err(|e| e.message)?;
        }
        if !shared.opts.quiet {
            let ratio = shared
                .opts
                .profile
                .as_ref()
                .map(|p| p.measurement.effective_ratio)
                .unwrap_or(0.0);
            log_line(
                &serde_json::json!({
                    "event": "recalibrated",
                    "model_id": model_id,
                    "reason": reason,
                    "ratio": ratio,
                }),
                false,
            );
        }
        Ok(())
    })();
    if let Err(err) = result {
        if !shared.opts.quiet {
            log_line(
                &serde_json::json!({
                    "event": "recalibrate_failed",
                    "model_id": model_id,
                    "reason": reason,
                    "error": err,
                }),
                true,
            );
        }
    }
    shared.in_flight.lock().unwrap().remove(model_id);
}

fn tick(shared: &Arc<Shared>, model_id: &str) {
    let count = {
        let mut ticks = shared.ticks.lock().unwrap();
        let count = ticks.get(model_id).copied().unwrap_or(0) + 1;
        if should_recalibrate(count) {
            ticks.insert(model_id.to_string(), 0);
        } else {
            ticks.insert(model_id.to_string(), count);
        }
        count
    };
    if should_recalibrate(count) {
        let shared_clone = Arc::clone(shared);
        let model = model_id.to_string();
        std::thread::spawn(move || calibrate_model(&shared_clone, &model, "periodic"));
    }
}

fn auth_header(req: &Request, opts: &ProxyOptions) -> Option<String> {
    if let Some(key) = opts.api_key.as_ref().filter(|k| !k.is_empty()) {
        return Some(if key.starts_with("Bearer ") {
            key.clone()
        } else {
            format!("Bearer {}", key)
        });
    }
    req.headers
        .get("authorization")
        .filter(|v| !v.is_empty())
        .cloned()
}

fn upstream_headers(req: &Request, opts: &ProxyOptions) -> Vec<(String, String)> {
    let mut headers: Vec<(String, String)> = Vec::new();
    headers.push(("Content-Type".to_string(), "application/json".to_string()));
    if let Some(key) = req.headers.get("api-key").filter(|v| !v.is_empty()) {
        headers.push(("api-key".to_string(), key.clone()));
    }
    if let Some(auth) = auth_header(req, opts) {
        if host_of(&opts.upstream).to_ascii_lowercase().ends_with(".openai.azure.com") {
            let stripped = if auth.to_ascii_lowercase().starts_with("bearer ") {
                auth[7..].trim().to_string()
            } else {
                auth.clone()
            };
            headers.push(("api-key".to_string(), stripped));
        } else {
            headers.push(("Authorization".to_string(), auth));
        }
    }
    headers
}

struct UpstreamResponse {
    status: u16,
    headers: Vec<(String, String)>,
    response: ureq::Response,
}

fn call_upstream(
    payload: &Value,
    headers: &[(String, String)],
    opts: &ProxyOptions,
    timeout_ms: u64,
) -> Result<UpstreamResponse, (bool, String)> {
    let Some(url) = target_url(&opts.upstream) else {
        return Err((false, "上游地址非法".to_string()));
    };
    let endpoint = format!("{}/chat/completions", url.trim_end_matches('/'));
    let agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(timeout_ms))
        .build();
    let mut request = agent.post(&endpoint);
    for (k, v) in headers {
        request = request.set(k, v);
    }
    match request.send_json(payload.clone()) {
        Ok(response) => {
            let status = response.status();
            let captured = response
                .headers_names()
                .into_iter()
                .filter_map(|n| response.header(&n).map(|v| (n.to_string(), v.to_string())))
                .collect();
            Ok(UpstreamResponse {
                status,
                headers: captured,
                response,
            })
        }
        Err(ureq::Error::Status(status, response)) => {
            let captured = response
                .headers_names()
                .into_iter()
                .filter_map(|n| response.header(&n).map(|v| (n.to_string(), v.to_string())))
                .collect();
            Ok(UpstreamResponse {
                status,
                headers: captured,
                response,
            })
        }
        Err(ureq::Error::Transport(t)) => {
            let text = format!("{}", t);
            let lower = text.to_ascii_lowercase();
            Err((
                lower.contains("timed out") || lower.contains("timeout"),
                text,
            ))
        }
    }
}

fn upstream_content_type(headers: &[(String, String)], fallback: &str) -> String {
    headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-type"))
        .map(|(_, v)| v.clone())
        .unwrap_or_else(|| fallback.to_string())
}

fn handle_chat(shared: &Arc<Shared>, stream: &mut TcpStream, req: &Request, cors: Option<&str>) {
    let started = Instant::now();
    let opts = &shared.opts;
    let body: Value = match serde_json::from_slice(&req.body) {
        Ok(v) => v,
        Err(_) => {
            send_error(stream, 400, cors, "invalid_request_error", "请求体不是合法 JSON");
            return;
        }
    };
    let messages: Option<Vec<Value>> = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned();
    let Some(messages) = messages else {
        send_error(stream, 400, cors, "invalid_request_error", "缺少 messages 数组");
        return;
    };

    let raw = last_user_text(&messages);
    let body_model = body.get("model").and_then(Value::as_str).map(|s| s.to_string());
    let model_id = body_model
        .clone()
        .unwrap_or_else(|| opts.model.clone());
    let profile = match &opts.profile {
        Some(p) if body_model.is_none() || body_model.as_deref() == Some(p.model.model_id.as_str()) => {
            Some(p.clone())
        }
        _ => resolve_profile(&model_id),
    };
    let model = meta_for(&model_id, profile.as_ref().map(|p| p.measurement.effective_ratio));
    let signals = extract_signals(&raw, &history_text(&messages));
    let r = estimate_r(signals.task, &model);
    let is_stream = body.get("stream").and_then(Value::as_bool) == Some(true);
    let decision = route(&RouteInput {
        signals: &signals,
        model: &model,
        profile: profile.as_ref(),
        r,
        streaming: is_stream,
        user_system_prompt: Some(system_text(&messages).as_str()).filter(|s| !s.is_empty()),
        now_ms: now_ms(),
    });

    let forwarded = with_injection(&messages, &decision.injection);
    let mut payload = body.clone();
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("messages".to_string(), Value::Array(forwarded));
    }

    let timeout_ms = if opts.upstream_timeout_ms > 0 {
        opts.upstream_timeout_ms
    } else {
        DEFAULT_UPSTREAM_TIMEOUT_MS
    };

    let upstream = match call_upstream(&payload, &upstream_headers(req, opts), opts, timeout_ms) {
        Ok(u) => u,
        Err((timeout, message)) => {
            if timeout {
                send_error(stream, 504, cors, "upstream_timeout", "上游超时");
            } else {
                send_error(
                    stream,
                    502,
                    cors,
                    "upstream_error",
                    &format!("上游请求失败：{}", message),
                );
            }
            return;
        }
    };

    tick(shared, &model_id);
    let request_id = new_request_id();
    let mut base_headers: Vec<(String, String)> = cors.map(cors_headers).unwrap_or_default();
    base_headers.push(("x-langbridge-request-id".to_string(), request_id));
    base_headers.push((
        "x-langbridge-reasoning-lang".to_string(),
        lang_code(decision.routing.decided_lang),
    ));
    base_headers.push((
        "x-langbridge-decided-by".to_string(),
        decided_by_code(decision.routing.decided_by),
    ));
    base_headers.push((
        "x-langbridge-injection".to_string(),
        injection_state_code(decision.injection.state),
    ));
    if let Some(savings) = &decision.savings {
        base_headers.push(("x-langbridge-net-tokens".to_string(), js_num(savings.net)));
    }

    if is_stream {
        let content_type = upstream_content_type(&upstream.headers, "text/event-stream; charset=utf-8");
        let mut headers = base_headers;
        headers.push(("Content-Type".to_string(), content_type));
        headers.push(("Cache-Control".to_string(), "no-cache".to_string()));
        headers.push(("Transfer-Encoding".to_string(), "chunked".to_string()));
        let status = upstream.status;
        let mut head = format!("HTTP/1.1 {} {}\r\n", status, reason_phrase(status));
        for (k, v) in &headers {
            head.push_str(&format!("{}: {}\r\n", k, v));
        }
        head.push_str("\r\n");
        if stream.write_all(head.as_bytes()).is_err() {
            return;
        }
        let mut reader = upstream.response.into_reader();
        let mut buf = vec![0u8; 8192];
        let mut raw_stream: Vec<u8> = Vec::new();
        let mut client_gone = false;
        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if raw_stream.len() + n > MAX_STREAM_BUFFER {
                        let allowed = MAX_STREAM_BUFFER.saturating_sub(raw_stream.len());
                        raw_stream.extend_from_slice(&buf[..allowed]);
                        let error_chunk = format!(
                            "data: {}\n\n",
                            serde_json::json!({ "error": { "type": "stream_error", "message": "流式响应超过 8MB，已截断" } })
                        );
                        let _ = write_chunk(stream, error_chunk.as_bytes());
                        break;
                    }
                    raw_stream.extend_from_slice(&buf[..n]);
                    if write_chunk(stream, &buf[..n]).is_err() {
                        client_gone = true;
                        break;
                    }
                }
                Err(_) => {
                    let error_chunk = format!(
                        "data: {}\n\n",
                        serde_json::json!({ "error": { "type": "upstream_error", "message": "上游连接中断" } })
                    );
                    let _ = write_chunk(stream, error_chunk.as_bytes());
                    break;
                }
            }
        }
        if !client_gone {
            let _ = stream.write_all(b"0\r\n\r\n");
            let _ = stream.flush();
        }
        let raw_text = String::from_utf8_lossy(&raw_stream).into_owned();
        let streamed = sse_text(&raw_text);
        if !streamed.is_empty() {
            let _ = inspect_only(&streamed, decision.routing.output_lang);
        }
    } else {
        let text = match upstream.response.into_string() {
            Ok(t) => t,
            Err(_) => {
                send_error(stream, 502, cors, "upstream_error", "上游响应读取失败");
                return;
            }
        };
        let mut out = text.clone();
        if let Ok(mut parsed) = serde_json::from_str::<Value>(&text) {
            let maybe_content = parsed
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            if let Some(content) = maybe_content {
                let guarded = postprocess(
                    &content,
                    &PostprocessOptions {
                        expected_lang: decision.routing.output_lang,
                        streaming: false,
                    },
                );
                if let Some(message) = parsed
                    .get_mut("choices")
                    .and_then(|c| c.get_mut(0))
                    .and_then(|c| c.get_mut("message"))
                    .and_then(Value::as_object_mut)
                {
                    message.insert("content".to_string(), Value::String(guarded.answer.clone()));
                    if !guarded.thinking.is_empty() {
                        message.insert(
                            "reasoning_content".to_string(),
                            Value::String(guarded.thinking.clone()),
                        );
                    }
                }
                out = serde_json::to_string(&parsed).unwrap_or(text);
            }
        }
        let mut headers = base_headers;
        headers.push((
            "Content-Type".to_string(),
            upstream_content_type(&upstream.headers, "application/json; charset=utf-8"),
        ));
        let passthrough = regex::Regex::new(r"(?i)^(?:retry-after|x-ratelimit-[\w-]+)$").unwrap();
        for (name, value) in &upstream.headers {
            if passthrough.is_match(name) {
                headers.push((name.clone(), value.clone()));
            }
        }
        if write_response(stream, upstream.status, &headers, out.as_bytes()).is_err() {
            return;
        }
    }

    log_summary(
        opts,
        &model_id,
        &signals,
        &decision.routing,
        decision.savings.as_ref(),
        Some(&decision.injection),
        started.elapsed().as_millis() as u64,
    );
}

fn lang_code(lang: langbridge_core::Lang) -> String {
    match lang {
        langbridge_core::Lang::Zh => "zh".to_string(),
        langbridge_core::Lang::En => "en".to_string(),
    }
}

fn decided_by_code(decided_by: langbridge_core::DecidedBy) -> String {
    serde_json::to_value(decided_by)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "L4".to_string())
}

fn injection_state_code(state: langbridge_core::InjectionState) -> String {
    crate::format::injection_state_code(state).to_string()
}

fn log_summary(
    opts: &ProxyOptions,
    model_id: &str,
    signals: &Signals,
    routing: &Routing,
    savings: Option<&SavingsResult>,
    injection: Option<&InjectionPlan>,
    duration_ms: u64,
) {
    if opts.quiet {
        return;
    }
    let summary = summarize_decision(
        model_id,
        signals,
        routing,
        savings,
        injection,
        Some(duration_ms),
        &iso_from_ms(now_ms()),
    );
    println!("{}", serde_json::to_string(&summary).unwrap_or_default());
}

fn handle_connection(mut stream: TcpStream, shared: Arc<Shared>) {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(300)));

    let req = match read_request(&stream) {
        Ok(r) => r,
        Err(ReadError::TooLarge) => {
            send_error(&mut stream, 413, None, "payload_too_large", "请求体超过 8MB");
            return;
        }
        Err(ReadError::Bad(reason)) => {
            if !shared.opts.quiet {
                eprintln!("请求体读取失败：{}", reason);
            }
            send_error(&mut stream, 400, None, "invalid_request_error", "请求体读取失败");
            return;
        }
        Err(ReadError::Closed) => return,
    };

    let opts = &shared.opts;
    let origin_header = req.headers.get("origin").cloned();
    let trusted_origin = match &origin_header {
        Some(origin) if is_allowed_origin(origin, &opts.allow_origins) => Some(origin.clone()),
        _ => None,
    };
    if origin_header.is_some() && trusted_origin.is_none() {
        send_error(&mut stream, 403, None, "forbidden_origin", "不允许的 Origin");
        return;
    }
    if !is_allowed_host(req.headers.get("host").map(|s| s.as_str()), &opts.allow_hosts) {
        send_error(&mut stream, 403, None, "forbidden_host", "不允许的 Host");
        return;
    }
    let cors = trusted_origin.as_deref();

    if req.method == "OPTIONS" {
        let mut headers: Vec<(String, String)> = cors.map(cors_headers).unwrap_or_default();
        if cors.is_none() {
            headers.push(("Access-Control-Allow-Headers".to_string(), ALLOW_HEADERS.to_string()));
            headers.push(("Access-Control-Allow-Methods".to_string(), "POST, GET, OPTIONS".to_string()));
        }
        let _ = write_response(&mut stream, 204, &headers, b"");
        return;
    }

    let path = req
        .url
        .split('?')
        .next()
        .unwrap_or("/")
        .trim_end_matches('/')
        .to_string();

    if req.method == "GET" && req.url == "/health" {
        let upstream_host = url::Url::parse(&target_url(&opts.upstream).unwrap_or_default())
            .ok()
            .map(|u| u[url::Position::BeforeHost..url::Position::AfterPort].to_string())
            .unwrap_or_default();
        let model = if !opts.model.is_empty() {
            opts.model.clone()
        } else {
            opts.profile
                .as_ref()
                .map(|p| p.model.model_id.clone())
                .unwrap_or_else(|| "deepseek-reasoner".to_string())
        };
        let payload = serde_json::json!({ "ok": true, "upstreamHost": upstream_host, "model": model });
        let _ = send_json(&mut stream, 200, cors, &payload);
        return;
    }

    let is_models_path = path == "/models" || path.ends_with("/models");
    if is_models_path {
        if req.method != "GET" {
            let mut headers = cors.map(cors_headers).unwrap_or_default();
            headers.push(("Allow".to_string(), "GET, OPTIONS".to_string()));
            let payload = serde_json::json!({ "error": { "message": "仅支持 GET /v1/models", "type": "method_not_allowed" } });
            let body = serde_json::to_vec(&payload).unwrap_or_default();
            headers.push(("Content-Type".to_string(), "application/json; charset=utf-8".to_string()));
            let _ = write_response(&mut stream, 405, &headers, &body);
            return;
        }
        let mut ids: HashSet<String> = list_models().into_iter().map(|m| m.model_id).collect();
        if !opts.model.is_empty() {
            ids.insert(opts.model.clone());
        }
        if let Some(p) = &opts.profile {
            ids.insert(p.model.model_id.clone());
        }
        let data: Vec<Value> = ids
            .into_iter()
            .map(|id| serde_json::json!({ "id": id, "object": "model", "created": 0, "owned_by": "langbridge" }))
            .collect();
        let payload = serde_json::json!({ "object": "list", "data": data });
        let _ = send_json(&mut stream, 200, cors, &payload);
        return;
    }

    if req.method == "POST" && req.url.split('?').next().unwrap_or("").ends_with("/chat/completions") {
        handle_chat(&shared, &mut stream, &req, cors);
        return;
    }

    send_error(
        &mut stream,
        404,
        cors,
        "not_found",
        "仅支持 POST /v1/chat/completions、GET /v1/models 与 GET /health",
    );
}

pub fn start_proxy(opts: ProxyOptions) -> Result<RunningProxy, String> {
    target_url(&opts.upstream).ok_or_else(|| "上游地址非法：需要合法的 http(s) 地址".to_string())?;
    let listener = TcpListener::bind((opts.host.as_str(), opts.port))
        .map_err(|e| format!("监听 {}:{} 失败：{}", opts.host, opts.port, e))?;
    let local = listener
        .local_addr()
        .map_err(|e| format!("读取监听地址失败：{}", e))?;

    let shared = Arc::new(Shared {
        ticks: Mutex::new(HashMap::new()),
        in_flight: Mutex::new(HashSet::new()),
        opts: opts.clone(),
    });

    if !opts.model.is_empty()
        && opts
            .profile
            .as_ref()
            .map(|p| p.model.model_id != opts.model)
            .unwrap_or(true)
        && !has_valid_profile(&opts.model)
    {
        let shared_clone = Arc::clone(&shared);
        let model = opts.model.clone();
        std::thread::spawn(move || calibrate_model(&shared_clone, &model, "startup"));
    }

    let handle = std::thread::spawn(move || {
        for incoming in listener.incoming() {
            match incoming {
                Ok(stream) => {
                    let shared = Arc::clone(&shared);
                    std::thread::spawn(move || handle_connection(stream, shared));
                }
                Err(_) => continue,
            }
        }
    });

    Ok(RunningProxy {
        url: format!("http://{}:{}", opts.host, local.port()),
        handle,
    })
}
