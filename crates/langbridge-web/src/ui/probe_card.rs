//! API 探测卡：服务商与 Key、连接测试、进度与结果对照。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use langbridge_core::evidence::calibrate_from_evidence;

use crate::copy::{FLOW, PROBE, fmt2};
use crate::probe::{ConnectionCheck, code_str};
use crate::state::{AppCtx, CalibrationStatus, ProbeLogStatus};

#[component]
pub fn ApiProbe() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let state = move || ctx.state.get();

    let base_url = move || state().probe_base_url;
    let preset_for_url = move |url: &str| {
        let target = normalize_base(url);
        PROBE.presets.iter().find(|p| normalize_base(p.base_url) == target)
    };
    let provider_value = move || {
        preset_for_url(&base_url())
            .map(|p| p.name.to_string())
            .unwrap_or_else(|| PROBE.provider_custom.to_string())
    };

    let initial_model = move || state().model_id;
    let model_id = RwSignal::new(initial_model());
    let api_key = RwSignal::new(String::new());
    let show_key = RwSignal::new(false);
    let endpoint_open = RwSignal::new(false);
    let missing = RwSignal::new(None::<&'static str>);
    let checking = RwSignal::new(false);
    let check = RwSignal::new(None::<ConnectionCheck>);
    let check_seq = RwSignal::new(0u32);

    let running = move || state().calibration.status == CalibrationStatus::Running;

    let invalidate_check = move || {
        check_seq.update(|seq| *seq = seq.wrapping_add(1));
        check.set(None);
        checking.set(false);
        missing.set(None);
    };

    let global_model = Memo::new(move |_| state().model_id);
    let last_model = RwSignal::new(global_model.get_untracked());
    Effect::new(move |_| {
        let id = global_model.get();
        if id == last_model.get_untracked() {
            return;
        }
        last_model.set(id.clone());
        model_id.set(id);
        invalidate_check();
    });

    let on_provider = move |ev: leptos::ev::Event| {
        let name = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok())
            .map(|el| el.value())
            .unwrap_or_default();
        if name == PROBE.provider_custom {
            endpoint_open.set(true);
            invalidate_check();
            return;
        }
        let Some(preset) = PROBE.presets.iter().find(|p| p.name == name) else {
            return;
        };
        ctx.set_probe_base_url(preset.base_url.to_string());
        model_id.set(preset.model_id.to_string());
        invalidate_check();
    };

    let on_endpoint_toggle = move |_| endpoint_open.update(|open| *open = !*open);

    let run = move |_| {
        if running() {
            ctx.cancel_probe();
            return;
        }
        let base = ctx.state.get_untracked().probe_base_url;
        let model = model_id.get_untracked();
        let key = api_key.get_untracked();
        if let Some(field) = missing_field(&base, &model, &key) {
            missing.set(Some(field));
            return;
        }
        invalidate_check();
        ctx.calibrate_api(base, key, model);
    };

    let test = move |_| {
        let base = ctx.state.get_untracked().probe_base_url;
        let model = model_id.get_untracked();
        let key = api_key.get_untracked();
        if let Some(field) = missing_field(&base, &model, &key) {
            missing.set(Some(field));
            return;
        }
        invalidate_check();
        let seq = check_seq.get_untracked();
        checking.set(true);
        let ctx = ctx;
        wasm_bindgen_futures::spawn_local(async move {
            let result = ctx.check_connection_async(base, key, model).await;
            if check_seq.get_untracked() == seq {
                check.set(Some(result));
                checking.set(false);
            }
        });
    };

    let source_label = move || match state().profile {
        None => PROBE.source_none,
        Some(profile)
            if profile.measurement.method
                == langbridge_core::types::ProfileSource::ApiProbe =>
        {
            PROBE.source_api
        }
        Some(_) => PROBE.source_builtin,
    };

    let measured_now = move || {
        let id = model_id.get().trim().to_string();
        state().measured.iter().any(|entry| entry.id == id)
    };
    let phase = move || {
        if measured_now() {
            5
        } else if check.get().map(|result| result.ok).unwrap_or(false) {
            4
        } else if !api_key.get().trim().is_empty() {
            3
        } else {
            2
        }
    };

    let calibration = move || state().calibration;
    let pct = move || {
        let step = calibration().step;
        match step {
            Some(step) if step.total > 0 => {
                (step.done as f64 / step.total as f64 * 100.0).round() as u32
            }
            _ => 0,
        }
    };

    view! {
        <section class="card probe" id="api-probe" data-testid="api-probe" aria-busy=move || {
            if running() { "true" } else { "false" }
        }>
            <header class="probe-head">
                <div>
                    <span class="verdict-kicker">{PROBE.title}</span>
                    <p class="probe-intro">{PROBE.intro}</p>
                </div>
                <span class="probe-source" data-testid="probe-source">{move || source_label()}</span>
            </header>

            <ol class="probe-steps" data-testid="probe-steps">
                {FLOW
                    .probe_steps
                    .iter()
                    .enumerate()
                    .map(|(index, label)| {
                        let no = index + 1;
                        let state_class = move || {
                            if phase() > no {
                                "probe-step done"
                            } else if phase() == no {
                                "probe-step current"
                            } else {
                                "probe-step"
                            }
                        };
                        view! {
                            <li class=state_class>
                                <span class="probe-step-no">{no}</span>
                                <span class="probe-step-label">{*label}</span>
                            </li>
                        }
                    })
                    .collect_view()}
            </ol>

            <div class="probe-config">
                <div class="probe-fields">
                    <label class="probe-provider">
                        <span class="probe-label">{PROBE.provider}</span>
                        <select
                            class="probe-select"
                            aria-label=PROBE.provider_aria
                            on:change=on_provider
                            prop:value=move || provider_value()
                        >
                            {PROBE
                                .presets
                                .iter()
                                .map(|preset| {
                                    view! { <option value=preset.name>{preset.name}</option> }
                                })
                                .collect_view()}
                            <option value=PROBE.provider_custom>{PROBE.provider_custom}</option>
                        </select>
                    </label>
                    <label class="probe-key">
                        <span class="probe-label">
                            {PROBE.api_key}
                            <button
                                type="button"
                                class="key-toggle"
                                aria-pressed=move || if show_key.get() { "true" } else { "false" }
                                on:click=move |_| show_key.update(|v| *v = !*v)
                            >
                                {move || if show_key.get() { PROBE.hide_key } else { PROBE.show_key }}
                            </button>
                        </span>
                        <input
                            type=move || if show_key.get() { "text" } else { "password" }
                            autocomplete="off"
                            spellcheck="false"
                            placeholder="sk-…"
                            prop:value=move || api_key.get()
                            on:input=move |ev| {
                                api_key.set(event_target_value(&ev));
                                invalidate_check();
                            }
                        />
                    </label>
                </div>

                <div class="probe-actions">
                    <button
                        type="button"
                        class="btn btn-ghost probe-test-btn"
                        disabled=move || running() || checking.get()
                        on:click=test
                    >
                        {move || if checking.get() { PROBE.preparing } else { "测试连接" }}
                    </button>
                    <button
                        type="button"
                        class="btn btn-primary probe-run-btn"
                        on:click=run
                    >
                        {move || {
                            if running() {
                                PROBE.cancel
                            } else if measured_now() {
                                PROBE.retry_run
                            } else {
                                "开始测量"
                            }
                        }}
                    </button>
                </div>

                <p class="probe-hint" data-testid="probe-next">
                    {move || {
                        if running() {
                            "正在测量，进度见下方。过程中可以随时取消。"
                        } else if measured_now() {
                            FLOW.probe_next_done
                        } else if check.get().map(|result| result.ok).unwrap_or(false) {
                            FLOW.probe_next_run
                        } else if !api_key.get().trim().is_empty() {
                            FLOW.probe_next_test
                        } else {
                            FLOW.probe_next_key
                        }
                    }}
                </p>

                <button
                    type="button"
                    class="probe-endpoint"
                    aria-expanded=move || if endpoint_open.get() { "true" } else { "false" }
                    on:click=on_endpoint_toggle
                >
                    <span class="probe-endpoint-text">
                        {move || endpoint_text(&base_url(), &model_id.get())}
                    </span>
                    <span class="probe-endpoint-action">
                        {move || if endpoint_open.get() { PROBE.endpoint_collapse } else { PROBE.endpoint_edit }}
                    </span>
                </button>

                <div class="probe-endpoint-fields" class:hidden=move || !endpoint_open.get()>
                    <label>
                        <span class="probe-label">{PROBE.base_url}</span>
                        <input
                            placeholder=PROBE.base_url_placeholder
                            prop:value=move || base_url()
                            on:input=move |ev| {
                                ctx.set_probe_base_url(event_target_value(&ev));
                                invalidate_check();
                            }
                        />
                    </label>
                    <label>
                        <span class="probe-label">{PROBE.model_id}</span>
                        <input
                            placeholder="deepseek-reasoner"
                            prop:value=move || model_id.get()
                            on:input=move |ev| {
                                model_id.set(event_target_value(&ev));
                                invalidate_check();
                            }
                        />
                    </label>
                </div>

                <p class="probe-note">{move || PROBE.run_note()}</p>

                {move || {
                    missing
                        .get()
                        .map(|field| {
                            view! {
                                <p class="notice notice-warn" data-testid="probe-preflight" role="status">
                                    {field}
                                </p>
                            }
                        })
                }}
                {move || {
                    check
                        .get()
                        .map(|result| {
                            let class = if result.ok { "probe-status ok" } else { "probe-status bad" };
                            let title = if result.ok { "连接正常" } else { "连接失败" };
                            let meta = if result.ok {
                                format!("{:.0} ms", result.latency_ms)
                            } else {
                                result.code.clone().unwrap_or_default()
                            };
                            view! {
                                <div class=class>
                                    <div class="probe-status-head">
                                        <span class="probe-status-dot" aria-hidden="true"></span>
                                        <span class="probe-status-title">{title}</span>
                                        <span class="probe-status-meta">{meta}</span>
                                    </div>
                                    <p class="probe-status-text">{result.message.clone()}</p>
                                </div>
                            }
                        })
                }}
            </div>

            <div class="progress-wrap" class:hidden=move || !running()>
                <div
                    class="progress"
                    role="progressbar"
                    aria-valuemin="0"
                    aria-valuemax="100"
                    aria-valuenow=move || pct()
                    aria-label=move || {
                        PROBE.progress_aria(
                            &calibration().step.map(|s| s.label).unwrap_or_default(),
                        )
                    }
                >
                    <span class="progress-fill" style=move || format!("width: {}%", pct())></span>
                </div>
                <div class="progress-meta">
                    <span>{move || calibration().step.map(|s| s.label).unwrap_or_default()}</span>
                    <button
                        type="button"
                        class="probe-inline-action"
                        on:click=move |_| ctx.cancel_probe()
                    >
                        {PROBE.cancel}
                    </button>
                </div>
            </div>

            {move || match calibration().status {
                CalibrationStatus::Error => {
                    let calibration = calibration();
                    let code = calibration
                        .error
                        .as_ref()
                        .map(|e| code_str(e.code))
                        .unwrap_or_default();
                    let message = calibration
                        .error
                        .as_ref()
                        .map(|e| e.message.clone())
                        .unwrap_or_default();
                    let hint = calibration.hint.unwrap_or_default();
                    view! {
                        <div class="probe-status bad" data-testid="probe-error">
                            <div class="probe-status-head">
                                <span class="probe-status-dot" aria-hidden="true"></span>
                                <span class="probe-status-title">{PROBE.fail_title(&code)}</span>
                            </div>
                            <p class="probe-status-text">{message}</p>
                            <p class="probe-status-hint">{hint}</p>
                            <button type="button" class="probe-inline-action" on:click=run>
                                {PROBE.retry_run}
                            </button>
                        </div>
                    }
                        .into_any()
                }
                CalibrationStatus::Success => {
                    let calibration = calibration();
                    let measured = state()
                        .profile
                        .as_ref()
                        .map(|p| p.measurement.method == langbridge_core::types::ProfileSource::ApiProbe)
                        .unwrap_or(false);
                    if !measured {
                        return ().into_any();
                    }
                    let done = calibration.step.as_ref().map(|s| s.done).unwrap_or(0);
                    let total = calibration.step.as_ref().map(|s| s.total).unwrap_or(0);
                    view! {
                        <div class="probe-status ok" data-testid="probe-success">
                            <div class="probe-status-head">
                                <span class="probe-status-dot" aria-hidden="true"></span>
                                <span class="probe-status-title">{PROBE.ok_title}</span>
                                {(total > 0)
                                    .then(|| {
                                        view! {
                                            <span class="probe-status-meta">
                                                {format!("{}/{}", done, total)}
                                            </span>
                                        }
                                    })}
                            </div>
                            <p class="probe-status-text">{PROBE.compare_updated}</p>
                            <p class="probe-status-hint">{FLOW.probe_saved}</p>
                        </div>
                    }
                        .into_any()
                }
                _ => ().into_any(),
            }}

            {move || {
                let log = calibration().log;
                (!log.is_empty())
                    .then(|| {
                        view! {
                            <div class="probe-log-wrap">
                                <span class="probe-log-title">{PROBE.log_title}</span>
                                <ol class="probe-log" data-testid="probe-log">
                                    {log
                                        .into_iter()
                                        .map(|item| {
                                            let class = format!(
                                                "probe-log-item {}",
                                                match item.status {
                                                    ProbeLogStatus::Ok => "ok",
                                                    ProbeLogStatus::Fail => "fail",
                                                    ProbeLogStatus::Retry => "retry",
                                                    ProbeLogStatus::Info => "info",
                                                },
                                            );
                                            view! {
                                                <li class=class>
                                                    <span class="probe-log-dot" aria-hidden="true"></span>
                                                    <span>{item.text}</span>
                                                </li>
                                            }
                                        })
                                        .collect_view()}
                                </ol>
                            </div>
                        }
                    })
            }}

            <CompareRow />
        </section>
    }
}

#[component]
fn CompareRow() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let builtin = move || {
        let id = ctx.state.get().model_id;
        calibrate_from_evidence(&id)
            .ok()
            .map(|cal| cal.effective_ratio)
    };
    let measured = move || {
        ctx.state
            .get()
            .profile
            .as_ref()
            .filter(|p| p.measurement.method == langbridge_core::types::ProfileSource::ApiProbe)
            .map(|p| p.measurement.effective_ratio)
    };
    view! {
        <div class="probe-compare" data-testid="probe-compare">
            <div class="probe-compare-head">
                <span class="probe-compare-title">"和内置参考比一比"</span>
            </div>
            <div class="probe-compare-row">
                <span class="probe-compare-label">{PROBE.compare_builtin}</span>
                <span class="probe-compare-value">
                    {move || {
                        builtin()
                            .map(|value| format!("{}×", fmt2(value)))
                            .unwrap_or_else(|| PROBE.compare_no_baseline.to_string())
                    }}
                </span>
                <span class="probe-compare-label">{PROBE.compare_measured}</span>
                <span class="probe-compare-value">
                    {move || {
                        measured()
                            .map(|value| format!("{}×", fmt2(value)))
                            .unwrap_or_else(|| "未测".to_string())
                    }}
                </span>
            </div>
            <p class="probe-compare-note">
                {move || match (builtin(), measured()) {
                    (Some(base), Some(now)) => {
                        let delta = (now - base) / base * 100.0;
                        let direction = if delta >= 0.0 { "高" } else { "低" };
                        format!(
                            "实测比内置参考{} {}%，整页结论已改用实测值。",
                            direction,
                            fmt2(delta.abs())
                        )
                    }
                    (Some(base), None) => PROBE.compare_summary(Some(&fmt2(base))),
                    (None, _) => PROBE.compare_summary(None),
                }}
            </p>
        </div>
    }
}

fn normalize_base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

fn is_local(url: &str) -> bool {
    let trimmed = url.trim().to_ascii_lowercase();
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))
        .unwrap_or(&trimmed);
    ["localhost", "127.0.0.1", "0.0.0.0", "[::1]"]
        .iter()
        .any(|host| rest.starts_with(host))
}

fn missing_field(base: &str, model: &str, key: &str) -> Option<&'static str> {
    if base.trim().is_empty() {
        return Some(PROBE.need_base_url);
    }
    if model.trim().is_empty() {
        return Some(PROBE.need_model_id);
    }
    if key.trim().is_empty() && !is_local(base) {
        return Some(PROBE.need_api_key);
    }
    None
}

fn endpoint_text(base: &str, model: &str) -> String {
    let host = base.trim().trim_start_matches("https://").trim_start_matches("http://");
    [host, model.trim()].iter().filter(|s| !s.is_empty()).cloned().collect::<Vec<_>>().join(" · ")
}
