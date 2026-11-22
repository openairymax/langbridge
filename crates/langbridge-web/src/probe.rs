//! 浏览器 API 探测：fetch + AbortController + JsFuture + ReadableStream 手写。
//! 纯逻辑（URL、鉴权头、载荷、错误归类、usage 解析、差分校准）全部复用 core。

use std::cell::Cell;
use std::rc::Rc;

use js_sys::{Function, Reflect, Uint8Array};
use langbridge_core::evidence::{
    adapt_caps, build_payload, calibration_from_probe, classify_body_error, classify_http,
    is_retryable, parse_usage, probe_steps, request_target, step_sample, PayloadCaps, ProbeStep,
};
use langbridge_core::signals::slice_utf16;
use langbridge_core::types::{CalibrationResult, EngineError, ErrorCode, Lang, Observation};
use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    AbortController, AbortSignal, Headers, Request, RequestCache, RequestInit, RequestMode, Response,
};

const NETWORK_ERROR_MESSAGE: &str = "连不上：常见原因是跨域（CORS）被拦截，或网络不通";

/// 探测过程事件：每一步都上报，便于界面展示实时进度与失败原因。
#[derive(Debug, Clone)]
pub enum ProbeEvent {
    Start { total: usize },
    Step { step: ProbeStep },
    Retry { step: ProbeStep, attempt: u32, delay_ms: f64, message: String },
    Ok { step: ProbeStep, prompt_tokens: f64 },
    Fail { step: ProbeStep, error: EngineError, completed: usize, total: usize },
}

#[derive(Clone)]
pub struct ProbeOptions {
    pub base_url: String,
    pub api_key: String,
    pub model_id: String,
    pub base_n: Option<f64>,
    pub timeout_ms: Option<f64>,
    pub retries: Option<u32>,
    pub signal: Option<AbortSignal>,
    pub on_event: Option<Rc<dyn Fn(ProbeEvent)>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConnectionCheck {
    pub ok: bool,
    pub latency_ms: f64,
    pub message: String,
    pub code: Option<String>,
}

fn cancelled() -> EngineError {
    EngineError::new(ErrorCode::Cancelled, "已取消测量")
}

fn emit(opts: &ProbeOptions, event: ProbeEvent) {
    if let Some(cb) = &opts.on_event {
        cb(event);
    }
}

pub fn code_str(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "bad_response".to_string())
}

pub fn lang_str(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "zh",
        Lang::En => "en",
    }
}

fn js_num(value: f64) -> String {
    if value.fract() == 0.0 {
        format!("{:.0}", value)
    } else {
        format!("{}", value)
    }
}

fn is_aborted(signal: Option<&AbortSignal>) -> bool {
    signal.map(|s| s.aborted()).unwrap_or(false)
}

fn backoff_ms(attempt: u32) -> f64 {
    let base = 600.0 * 2.5_f64.powi(attempt as i32);
    (base + js_sys::Math::random() * 250.0).round().min(3000.0)
}

fn http_date_ms(raw: &str) -> Option<f64> {
    // RFC 1123: Wed, 21 Oct 2015 07:28:00 GMT
    let mut parts = raw.split_whitespace();
    let _weekday = parts.next()?;
    let day: i64 = parts.next()?.parse().ok()?;
    let month = match parts.next()? {
        "Jan" => 1,
        "Feb" => 2,
        "Mar" => 3,
        "Apr" => 4,
        "May" => 5,
        "Jun" => 6,
        "Jul" => 7,
        "Aug" => 8,
        "Sep" => 9,
        "Oct" => 10,
        "Nov" => 11,
        "Dec" => 12,
        _ => return None,
    };
    let year: i64 = parts.next()?.parse().ok()?;
    let mut clock = parts.next()?.split(':');
    let hour: i64 = clock.next()?.parse().ok()?;
    let minute: i64 = clock.next()?.parse().ok()?;
    let second: i64 = clock.next()?.parse().ok()?;
    let zone = parts.next().unwrap_or("GMT");
    if zone != "GMT" && zone != "UTC" {
        return None;
    }
    // days_from_civil
    let y = year - if month <= 2 { 1 } else { 0 };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let seconds = days * 86400 + hour * 3600 + minute * 60 + second;
    Some(seconds as f64 * 1000.0)
}

fn retry_after_ms(res: &Response) -> Option<f64> {
    let header = res.headers().get("retry-after").ok().flatten()?;
    if let Ok(seconds) = header.trim().parse::<f64>() {
        if seconds.is_finite() && seconds > 0.0 {
            return Some((seconds * 1000.0).round().min(8000.0));
        }
    }
    let date = http_date_ms(&header)?;
    let delta = date - js_sys::Date::now();
    if delta > 0.0 {
        Some(delta.min(8000.0))
    } else {
        None
    }
}

async fn sleep_cancellable(ms: f64, signal: Option<&AbortSignal>) -> Result<(), EngineError> {
    let mut left = ms.max(0.0);
    while left > 0.0 {
        if is_aborted(signal) {
            return Err(cancelled());
        }
        let chunk = left.min(50.0);
        gloo_timers::future::TimeoutFuture::new(chunk.max(1.0) as u32).await;
        left -= chunk;
    }
    Ok(())
}

struct CorsProbeState {
    probed: Cell<bool>,
}

async fn reachable_without_cors(url: &str) -> bool {
    let Some(win) = web_sys::window() else {
        return false;
    };
    let Ok(controller) = AbortController::new() else {
        return false;
    };
    let timer_controller = controller.clone();
    let _timer = gloo_timers::callback::Timeout::new(6000, move || {
        timer_controller.abort();
    });
    let init = RequestInit::new();
    init.set_method("GET");
    init.set_mode(RequestMode::NoCors);
    init.set_cache(RequestCache::NoStore);
    init.set_signal(Some(&controller.signal()));
    let Ok(request) = Request::new_with_str_and_init(url, &init) else {
        return false;
    };
    JsFuture::from(win.fetch_with_request(&request)).await.is_ok()
}

async fn classify_network_failure(
    url: &str,
    signal: Option<&AbortSignal>,
) -> EngineError {
    if is_aborted(signal) {
        return cancelled();
    }
    let offline = web_sys::window()
        .map(|w| w.navigator().on_line() == false)
        .unwrap_or(false);
    if offline {
        return EngineError::new(ErrorCode::NetworkError, NETWORK_ERROR_MESSAGE);
    }
    let reachable = reachable_without_cors(url).await;
    if is_aborted(signal) {
        return cancelled();
    }
    if reachable {
        EngineError::new(
            ErrorCode::CorsBlocked,
            "浏览器跨域被拦截：地址可达，但服务商不允许网页直连（CORS）",
        )
    } else {
        EngineError::new(ErrorCode::NetworkError, NETWORK_ERROR_MESSAGE)
    }
}

async fn read_stream_text(stream: &web_sys::ReadableStream) -> Result<String, String> {
    let get_reader = Reflect::get(stream.as_ref(), &JsValue::from_str("getReader"))
        .map_err(|_| "读取响应失败".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "读取响应失败".to_string())?;
    let reader = get_reader
        .call0(stream.as_ref())
        .map_err(|_| "读取响应失败".to_string())?;
    let read = Reflect::get(&reader, &JsValue::from_str("read"))
        .map_err(|_| "读取响应失败".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "读取响应失败".to_string())?;
    let mut bytes: Vec<u8> = Vec::new();
    loop {
        let promise = read
            .call0(&reader)
            .map_err(|_| "读取响应失败".to_string())?;
        let chunk = JsFuture::from(
            promise
                .dyn_into::<js_sys::Promise>()
                .map_err(|_| "读取响应失败".to_string())?,
        )
        .await
        .map_err(|_| "读取响应失败".to_string())?;
        let done = Reflect::get(&chunk, &JsValue::from_str("done"))
            .ok()
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if done {
            break;
        }
        let value = Reflect::get(&chunk, &JsValue::from_str("value"))
            .map_err(|_| "读取响应失败".to_string())?;
        if value.is_null() || value.is_undefined() {
            continue;
        }
        let array = Uint8Array::new(&value);
        let mut buffer = vec![0u8; array.length() as usize];
        array.copy_to(&mut buffer);
        bytes.extend_from_slice(&buffer);
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

async fn read_body_text(res: &Response) -> String {
    if let Some(body) = res.body() {
        if let Ok(text) = read_stream_text(&body).await {
            if !text.is_empty() {
                return text;
            }
        }
    }
    if let Ok(promise) = res.text() {
        if let Ok(value) = JsFuture::from(promise).await {
            return value.as_string().unwrap_or_default();
        }
    }
    String::new()
}

async fn read_server_message(res: &Response) -> String {
    let raw = read_body_text(res).await;
    if raw.is_empty() {
        return String::new();
    }
    if let Ok(body) = serde_json::from_str::<serde_json::Value>(&raw) {
        if let Some(error) = body.get("error") {
            if let Some(text) = error.as_str() {
                return slice_utf16(text, 200).to_string();
            }
            if error.is_object() {
                if let Some(message) = error.get("message").and_then(|v| v.as_str()) {
                    return slice_utf16(message, 200).to_string();
                }
            }
        }
        if let Some(message) = body.get("message").and_then(|v| v.as_str()) {
            return slice_utf16(message, 200).to_string();
        }
    }
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    slice_utf16(&collapsed, 200).to_string()
}

/// 单次请求：超时与外部取消合并到一个 AbortController 上。
async fn fetch_once(
    url: &str,
    headers: &Headers,
    body: &str,
    timeout_ms: f64,
    signal: Option<&AbortSignal>,
    cors: &CorsProbeState,
) -> Result<Response, EngineError> {
    if is_aborted(signal) {
        return Err(cancelled());
    }
    let controller =
        AbortController::new().map_err(|_| EngineError::new(ErrorCode::NetworkError, NETWORK_ERROR_MESSAGE))?;
    let timed_out = Rc::new(Cell::new(false));
    let timer_flag = Rc::clone(&timed_out);
    let timer_controller = controller.clone();
    let _timer = gloo_timers::callback::Timeout::new(timeout_ms.max(1.0) as u32, move || {
        timer_flag.set(true);
        timer_controller.abort();
    });

    let listener = signal.map(|sig| {
        let abort_controller = controller.clone();
        let callback = Closure::<dyn FnMut()>::new(move || {
            abort_controller.abort();
        });
        let _ = sig.add_event_listener_with_callback("abort", callback.as_ref().unchecked_ref());
        callback
    });

    let init = RequestInit::new();
    init.set_method("POST");
    init.set_headers(headers);
    init.set_body(&JsValue::from_str(body));
    init.set_mode(RequestMode::Cors);
    init.set_signal(Some(&controller.signal()));
    let request = Request::new_with_str_and_init(url, &init)
        .map_err(|_| EngineError::new(ErrorCode::NetworkError, NETWORK_ERROR_MESSAGE))?;

    let result = match web_sys::window() {
        Some(win) => JsFuture::from(win.fetch_with_request(&request)).await,
        None => Err(JsValue::from_str("no window")),
    };

    drop(_timer);
    if let (Some(sig), Some(callback)) = (signal, &listener) {
        let _ = sig.remove_event_listener_with_callback("abort", callback.as_ref().unchecked_ref());
    }

    match result {
        Ok(value) => value
            .dyn_into::<Response>()
            .map_err(|_| EngineError::new(ErrorCode::BadResponse, "返回的不是合法响应")),
        Err(_) => {
            if is_aborted(signal) {
                Err(cancelled())
            } else if timed_out.get() {
                Err(EngineError::new(
                    ErrorCode::Timeout,
                    format!("请求超时（{} 秒无响应）", (timeout_ms / 1000.0).round()),
                ))
            } else if !cors.probed.get() {
                cors.probed.set(true);
                Err(classify_network_failure(url, signal).await)
            } else {
                Err(EngineError::new(ErrorCode::NetworkError, NETWORK_ERROR_MESSAGE))
            }
        }
    }
}

async fn probe_prompt_tokens(
    opts: &ProbeOptions,
    text: &str,
    caps: &mut PayloadCaps,
    step: &ProbeStep,
) -> Result<f64, EngineError> {
    let target = request_target(&opts.base_url, &opts.api_key)?;
    let timeout_ms = opts.timeout_ms.unwrap_or(20000.0);
    let retries = opts.retries.unwrap_or(1);
    let mut attempt = 0u32;
    let mut adaptations = 3u32;
    let cors = CorsProbeState { probed: Cell::new(false) };

    loop {
        let headers = Headers::new()
            .map_err(|_| EngineError::new(ErrorCode::NetworkError, NETWORK_ERROR_MESSAGE))?;
        for (key, value) in &target.headers {
            let _ = headers.set(key, value);
        }
        let payload = build_payload(&opts.model_id, text, caps);
        let body = serde_json::to_string(&payload)
            .map_err(|e| EngineError::new(ErrorCode::BadRequest, e.to_string()))?;

        let res = fetch_once(
            &target.url,
            &headers,
            &body,
            timeout_ms,
            opts.signal.as_ref(),
            &cors,
        )
        .await?;

        if !res.ok() {
            let status = res.status();
            let server_message = read_server_message(&res).await;
            let error = classify_http(status, &server_message);
            if adaptations > 0
                && (status == 400 || status == 422)
                && adapt_caps(&server_message, caps)
            {
                adaptations -= 1;
                continue;
            }
            if is_retryable(error.code) && attempt < retries {
                let delay_ms = retry_after_ms(&res).unwrap_or_else(|| backoff_ms(attempt));
                attempt += 1;
                emit(
                    opts,
                    ProbeEvent::Retry {
                        step: step.clone(),
                        attempt,
                        delay_ms,
                        message: error.message.clone(),
                    },
                );
                sleep_cancellable(delay_ms, opts.signal.as_ref()).await?;
                continue;
            }
            return Err(error);
        }

        let raw = read_body_text(&res).await;
        let usage = parse_usage(&raw);
        if let Some(message) = usage.error_message {
            return Err(EngineError::new(
                classify_body_error(&message),
                format!("接口返回错误：{}", message),
            ));
        }
        if let Some(tokens) = usage.prompt_tokens {
            return Ok(tokens);
        }
        if !usage.parsed {
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

/// 连接自检：只发 1 个最小请求，判断 Key / Base URL / 模型 ID / 跨域。
pub async fn test_connection(opts: ProbeOptions) -> ConnectionCheck {
    let started = js_sys::Date::now();
    let step = ProbeStep {
        index: 1,
        total: 1,
        lang: Lang::En,
        n: 12.0,
    };
    let mut caps = PayloadCaps::default();
    let mut probe = opts.clone();
    probe.retries = Some(0);
    match probe_prompt_tokens(&probe, "ping", &mut caps, &step).await {
        Ok(tokens) => ConnectionCheck {
            ok: true,
            latency_ms: js_sys::Date::now() - started,
            message: format!("连接正常，返回 {} 个输入 token", js_num(tokens)),
            code: None,
        },
        Err(error) => ConnectionCheck {
            ok: false,
            latency_ms: js_sys::Date::now() - started,
            message: error.message,
            code: Some(code_str(error.code)),
        },
    }
}

/// 方法 B：API 探测。固定 6 个请求，逐个上报进度。
pub async fn calibrate_via_api(opts: ProbeOptions) -> Result<CalibrationResult, EngineError> {
    if langbridge_core::evidence::normalize_base(&opts.base_url).is_empty() {
        return Err(EngineError::new(ErrorCode::BadBaseUrl, "请填写 Base URL"));
    }
    if opts.model_id.trim().is_empty() {
        return Err(EngineError::new(ErrorCode::BadModel, "请填写模型 ID"));
    }

    let base_n = opts.base_n.unwrap_or(120.0);
    let steps = probe_steps(base_n);
    let total = steps.len();
    emit(&opts, ProbeEvent::Start { total });

    let mut observations: Vec<Observation> = Vec::new();
    let mut caps = PayloadCaps::default();

    for step in &steps {
        emit(
            &opts,
            ProbeEvent::Step {
                step: step.clone(),
            },
        );
        let text = step_sample(step, base_n);
        match probe_prompt_tokens(&opts, &text, &mut caps, step).await {
            Ok(prompt_tokens) => {
                observations.push(Observation {
                    sample_id: format!("{}-N{}", lang_str(step.lang), js_num(step.n)),
                    lang: step.lang,
                    n: step.n,
                    prompt_tokens,
                });
                emit(
                    &opts,
                    ProbeEvent::Ok {
                        step: step.clone(),
                        prompt_tokens,
                    },
                );
            }
            Err(error) => {
                emit(
                    &opts,
                    ProbeEvent::Fail {
                        step: step.clone(),
                        error: error.clone(),
                        completed: observations.len(),
                        total,
                    },
                );
                return Err(error);
            }
        }
    }

    let measured_at = js_sys::Date::new_0()
        .to_iso_string()
        .as_string()
        .unwrap_or_default();
    calibration_from_probe(&observations, &opts.model_id, measured_at)
}
