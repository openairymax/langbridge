//! 折叠区：参考数据与测量、数据来源与进阶、接入与高级。
//! 三段都默认收起，避免把首屏挤满；展开后按「结论 → 依据 → 原始数据」排序。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use langbridge_core::evidence::{evidence_meta, has_evidence, list_evidence_models};
use langbridge_core::guard::DRIFT_THRESHOLD;
use langbridge_core::profiles::get_model;
use langbridge_core::profile_store::{profile_file_name, serialize_profile};
use langbridge_core::types::InjectionState;

use crate::copy::{
    ADVANCED, CAL, EVIDENCE, FLOW, PROFILE, R_CUSTOM_ARIA, R_ROW_LABEL, SAVING_METRICS, SECTION,
    evidence_version_badge, fmt2, fmt3, lang_text, limitations, r_auto_label,
};
use crate::data::builtin_rows;
use crate::state::{
    AppCtx, CalibrationStatus, ProfileHealth, select_canonical, select_record,
};

fn reset_button_class(active: bool) -> &'static str {
    if active { "preset-chip active" } else { "preset-chip" }
}

#[component]
pub fn Folds() -> impl IntoView {
    view! {
        <div class="folds">
            <CalibrationFold />
            <EvidenceFold />
            <AdvancedFold />
        </div>
    }
}

/* ------------------------------------------------------------------ */
/* 参考数据与测量                                                      */
/* ------------------------------------------------------------------ */

#[component]
fn CalibrationFold() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let evaluation = move || ctx.evaluation();
    let profile = move || evaluation().profile;
    let rows = builtin_rows();
    let max_ratio = rows
        .iter()
        .map(|row| row.ratio)
        .fold(1.0_f64, f64::max);

    let auto_r = move || evaluation().r;
    let presets = move || crate::copy::R_PRESETS;

    let load_builtin = move |_| {
        let id = ctx.state.get_untracked().model_id;
        ctx.calibrate(Some(id));
    };

    view! {
        <details class="fold" id="fold-calibration">
            <summary class="fold-summary">{SECTION.calibration}</summary>
            <div class="fold-body">
                <p class="fold-lead">{FLOW.cal_lead}</p>

                <div class="cal-current" data-testid="cal-current">
                    <div class="cal-current-head">
                        <span class="cal-current-label">{FLOW.cal_current}</span>
                        <strong>{move || evaluation().model.display_name}</strong>
                    </div>
                    {move || match profile() {
                        None => {
                            view! {
                                <div class="cal-empty">
                                    <p class="muted small">{FLOW.cal_no_data}</p>
                                    <div class="cal-empty-actions">
                                        {move || {
                                            has_evidence(&evaluation().model.model_id)
                                                .then(|| {
                                                    view! {
                                                        <button
                                                            type="button"
                                                            class="btn btn-sm"
                                                            on:click=load_builtin
                                                        >
                                                            {CAL.badge_builtin}
                                                        </button>
                                                    }
                                                })
                                        }}
                                        <button
                                            type="button"
                                            class="btn btn-ghost btn-sm"
                                            on:click=move |_| crate::io::scroll_to("api-probe")
                                        >
                                            {FLOW.cal_go_probe}
                                        </button>
                                    </div>
                                </div>
                            }
                                .into_any()
                        }
                        Some(profile) => {
                            let builtin = profile.measurement.method
                                == langbridge_core::types::ProfileSource::Builtin;
                            let measured = profile.measurement.measured_at.clone();
                            let tokenizer = format!(
                                "{} {}",
                                profile.measurement.tokenizer.name,
                                profile.measurement.tokenizer.version,
                            );
                            let metrics = profile.measurement.clone();
                            let status = evaluation().profile_status;
                            view! {
                                <>
                                    <div class="cal-current-body">
                                        <div class="cal-ratio-big">
                                            <span class="cal-ratio-value">
                                                {format!("{}×", fmt3(metrics.effective_ratio))}
                                            </span>
                                            <span class="cal-ratio-caption">
                                                {CAL.metrics.ratio}
                                            </span>
                                        </div>
                                        <div class="cal-current-meta">
                                            <div class="cal-head">
                                                <span class="chip">
                                                    {if builtin { CAL.badge_builtin } else { CAL.badge_done }}
                                                </span>
                                                <span class="muted small">
                                                    {CAL.measured_at} {measured}
                                                </span>
                                            </div>
                                            <p class="muted small">
                                                {CAL.tokenizer_label} {tokenizer}
                                            </p>
                                        </div>
                                        <dl class="metric-grid metric-grid-inline">
                                            <div>
                                                <dt>{CAL.metrics.r2}</dt>
                                                <dd>{fmt3(metrics.r2)}</dd>
                                            </div>
                                            <div>
                                                <dt>{CAL.metrics.linear}</dt>
                                                <dd>
                                                    {if metrics.linearity_ok {
                                                        CAL.metrics.linear_ok.to_string()
                                                    } else {
                                                        CAL.metrics.linear_bad()
                                                    }}
                                                </dd>
                                            </div>
                                            <div>
                                                <dt>{CAL.metrics.confidence}</dt>
                                                <dd>{fmt3(metrics.confidence)}</dd>
                                            </div>
                                        </dl>
                                    </div>
                                    {match status.health {
                                        ProfileHealth::LowQuality => {
                                            Some(view! { <p class="notice notice-warn">{format!("{}{}", CAL.warn_prefix, CAL.metrics.linear_bad())}</p> })
                                        }
                                        _ => None,
                                    }}
                                </>
                            }
                                .into_any()
                        }
                    }}
                </div>

                <div class="cal-grid">
                    <div class="cal-panel">
                        <p class="sub-title">{FLOW.cal_left}</p>
                        <p class="muted small">
                            {CAL.gen_at} {evidence_meta().generated_at}
                        </p>
                        <ul class="ratio-bars">
                            {rows
                                .iter()
                                .map(|row| {
                                    let width = (row.ratio / max_ratio * 100.0).clamp(4.0, 100.0);
                                    let name = row.name.clone();
                                    let value = format!("{}×", fmt2(row.ratio));
                                    let id = row.id.clone();
                                    let active_id = row.id.clone();
                                    let class = move || {
                                        if ctx.state.get().model_id == active_id {
                                            "ratio-row active"
                                        } else {
                                            "ratio-row"
                                        }
                                    };
                                    view! {
                                        <li>
                                            <button
                                                type="button"
                                                class=class
                                                on:click=move |_| {
                                                    ctx.load_model_demo(id.clone());
                                                    crate::io::reveal_verdict_if_stacked();
                                                }
                                            >
                                                <span class="ratio-name">{name}</span>
                                                <span class="ratio-track">
                                                    <span
                                                        class="ratio-fill"
                                                        style=format!("width: {}%", width)
                                                    ></span>
                                                </span>
                                                <span class="ratio-value">{value}</span>
                                            </button>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                    </div>

                    <div class="cal-panel">
                        <p class="sub-title">{FLOW.cal_right}</p>
                        <ol class="cal-steps">
                            {FLOW
                                .cal_steps
                                .iter()
                                .enumerate()
                                .map(|(index, text)| {
                                    view! {
                                        <li>
                                            <span class="cal-step-no">{index + 1}</span>
                                            <span>{*text}</span>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ol>
                        <p class="muted small">{CAL.method_note}</p>
                    </div>
                </div>

                <div class="cost-panel">
                    <p class="sub-title">{crate::copy::SAVING_TITLE}</p>
                    <p class="r-row">
                        <span class="muted small">{R_ROW_LABEL}</span>
                        <button
                            type="button"
                            class=move || reset_button_class(ctx.state.get().r_override.is_none())
                            on:click=move |_| ctx.set_r_override(None)
                        >
                            {move || r_auto_label(auto_r())}
                        </button>
                        {presets()
                            .iter()
                            .map(|preset| {
                                let value = *preset;
                                let label = format!("{}", preset);
                                view! {
                                    <button
                                        type="button"
                                        class=move || {
                                            reset_button_class(
                                                ctx.state.get().r_override == Some(value),
                                            )
                                        }
                                        aria-label=R_CUSTOM_ARIA
                                        on:click=move |_| ctx.set_r_override(Some(value))
                                    >
                                        {label}
                                    </button>
                                }
                            })
                            .collect_view()}
                    </p>
                    {move || {
                        evaluation()
                            .savings
                            .map(|saving| {
                                view! {
                                    <dl class="savings-grid">
                                        <div>
                                            <dt>{SAVING_METRICS.gross}</dt>
                                            <dd>{fmt2(saving.gross)}</dd>
                                        </div>
                                        <div>
                                            <dt>{SAVING_METRICS.overhead}</dt>
                                            <dd>{fmt2(saving.overhead)}</dd>
                                        </div>
                                        <div>
                                            <dt>{SAVING_METRICS.net}</dt>
                                            <dd>{fmt2(saving.net)}</dd>
                                        </div>
                                        <div>
                                            <dt>{SAVING_METRICS.threshold}</dt>
                                            <dd>{fmt2(saving.threshold)}</dd>
                                        </div>
                                    </dl>
                                }
                            })
                    }}
                    <p class="muted small repro-line">
                        {move || CAL.repro_cmd(&evaluation().model.model_id)}
                    </p>
                </div>
            </div>
        </details>
    }
}

/* ------------------------------------------------------------------ */
/* 数据来源与进阶                                                      */
/* ------------------------------------------------------------------ */

#[component]
fn EvidenceFold() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let covered = list_evidence_models();
    let covered_count = covered.len();
    let current = move || ctx.state.get().model_id;
    let limits = limitations(covered_count);
    let version = evidence_version_badge(evidence_meta().version);
    let generated = evidence_meta().generated_at.clone();
    let meta = evidence_meta();
    let tokenizer_line = EVIDENCE.tokenizer_mixed();

    let export = move |_| {
        if let Some(profile) = ctx.state.get_untracked().profile {
            let name = profile_file_name(&profile);
            let text = serialize_profile(&profile);
            crate::io::download_text(&name, &text, "application/json");
        }
    };

    let import = move |ev: leptos::ev::Event| {
        let Some(input) = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlInputElement>().ok())
        else {
            return;
        };
        let Some(file) = input.files().and_then(|files| files.get(0)) else {
            return;
        };
        // 清空选择框，同一个文件可以再次导入。
        input.set_value("");
        let ctx = ctx;
        wasm_bindgen_futures::spawn_local(async move {
            match crate::io::read_file_text(&file).await {
                Ok(text) => {
                    ctx.import_profile(&text);
                    crate::io::reveal_verdict_if_stacked();
                }
                Err(_) => ctx.set_storage_warning(Some(PROFILE.read_fail.to_string())),
            }
        });
    };

    // 删除是两步确认，且确认只对当前模型有效，切换模型后自动失效。
    let confirm_delete = RwSignal::new(None::<String>);
    let reset = move |_| {
        let state = ctx.state.get_untracked();
        if state.profile.is_none() {
            return;
        }
        if confirm_delete.get_untracked().as_deref() == Some(state.model_id.as_str()) {
            ctx.reset_profile();
            confirm_delete.set(None);
            return;
        }
        confirm_delete.set(Some(state.model_id));
        gloo_timers::callback::Timeout::new(4_000, move || confirm_delete.set(None)).forget();
    };
    let has_profile = move || ctx.state.get().profile.is_some();

    let show_raw = move || ctx.state.get().show_raw;
    let canonical = move || {
        let evaluation = ctx.evaluation();
        serde_json::to_string_pretty(&select_canonical(&ctx.state.get(), &evaluation))
            .unwrap_or_default()
    };
    let record = move || {
        let evaluation = ctx.evaluation();
        serde_json::to_string_pretty(&select_record(&ctx.state.get(), &evaluation))
            .unwrap_or_default()
    };

    view! {
        <details class="fold" id="fold-evidence">
            <summary class="fold-summary">{SECTION.evidence}</summary>
            <div class="fold-body">
                <p class="fold-lead">{FLOW.evidence_lead}</p>
                <p class="group-label">{FLOW.evidence_groups[0]}</p>
                <div class="evidence-head">
                    <span class="chip">{version}</span>
                    <span class="muted small">{EVIDENCE.gen_at} {generated}</span>
                    <span class="muted small">{tokenizer_line}</span>
                </div>

                <div class="evidence-grid">
                    <div class="evidence-panel">
                        <p class="group-label">{FLOW.evidence_groups[1]}</p>
                        <p class="sub-title">{EVIDENCE.covered_summary}</p>
                        <ul class="covered-list">
                            {covered
                                .iter()
                                .map(|id| {
                                    let name = get_model(id)
                                        .map(|m| m.display_name)
                                        .unwrap_or_else(|| id.clone());
                                    let id = id.clone();
                                    let id_for_class = id.clone();
                                    let id_for_click = id.clone();
                                    let id_for_mark = id.clone();
                                    view! {
                                        <li class=move || {
                                            if current() == id_for_class { "covered-row current" } else { "covered-row" }
                                        }>
                                            <button
                                                type="button"
                                                class="covered-btn"
                                                on:click=move |_| ctx.load_model_demo(id_for_click.clone())
                                            >
                                                <span>{name}</span>
                                                {move || {
                                                    (current() == id_for_mark)
                                                        .then(|| view! { <span class="muted small">{EVIDENCE.current_mark}</span> })
                                                }}
                                            </button>
                                        </li>
                                    }
                                })
                                .collect_view()}
                        </ul>
                        <p class="muted small">
                            {if covered.iter().any(|id| *id == current()) {
                                EVIDENCE.covered_ok
                            } else {
                                EVIDENCE.covered_miss
                            }}
                        </p>
                        <p class="muted small">{EVIDENCE.sha_label} {meta.sample_set_sha256.clone()}</p>
                    </div>

                    <div class="evidence-panel">
                        <p class="group-label">{FLOW.evidence_groups[2]}</p>
                        <p class="sub-title">{EVIDENCE.limit_summary}</p>
                        <ul class="limit-list">
                            {limits
                                .iter()
                                .map(|item| view! { <li>{item.clone()}</li> })
                                .collect_view()}
                        </ul>
                        <p class="group-label">{FLOW.evidence_groups[3]}</p>
                        <p class="muted small">{EVIDENCE.repro_summary}</p>
                        <code class="code-line">{EVIDENCE.repro_smoke}</code>
                    </div>
                </div>

                <p class="group-label">{FLOW.evidence_groups[4]}</p>
                <p class="muted small">{FLOW.profile_note}</p>
                <div class="profile-actions">
                    <button
                        type="button"
                        class="btn btn-sm"
                        disabled=move || !has_profile()
                        on:click=export
                    >
                        "导出当前画像"
                    </button>
                    <label class="btn btn-sm btn-ghost file-label">
                        "导入画像"
                        <input type="file" accept=".json,application/json" on:change=import />
                    </label>
                    <button
                        type="button"
                        class="btn btn-sm btn-ghost"
                        disabled=move || !has_profile()
                        on:click=reset
                    >
                        {move || {
                            let id = ctx.state.get().model_id;
                            if confirm_delete.get().as_deref() == Some(id.as_str()) {
                                "再点一次确认删除"
                            } else {
                                "删除画像"
                            }
                        }}
                    </button>
                </div>

                {move || {
                    ctx.state
                        .get()
                        .import_errors
                        .as_ref()
                        .map(|errors| {
                            view! {
                                <ul class="import-errors">
                                    {errors
                                        .iter()
                                        .map(|error| {
                                            view! {
                                                <li>{format!("{}：{}", error.path, error.message)}</li>
                                            }
                                        })
                                        .collect_view()}
                                </ul>
                            }
                        })
                }}

                <button
                    type="button"
                    class="btn btn-sm btn-ghost raw-toggle"
                    aria-expanded=move || if show_raw() { "true" } else { "false" }
                    on:click=move |_| ctx.toggle_raw()
                >
                    {if show_raw() { "收起原始数据" } else { "查看原始数据" }}
                </button>
                {move || {
                    show_raw()
                        .then(|| {
                            view! {
                                <div class="raw-panels">
                                    <p class="sub-title">{EVIDENCE.canonical_summary}</p>
                                    <pre class="json-block">{canonical()}</pre>
                                    <p class="sub-title">{EVIDENCE.record_summary}</p>
                                    <pre class="json-block">{record()}</pre>
                                </div>
                            }
                        })
                }}
            </div>
        </details>
    }
}

/* ------------------------------------------------------------------ */
/* 接入与高级                                                          */
/* ------------------------------------------------------------------ */

#[component]
fn AdvancedFold() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let evaluation = move || ctx.evaluation();
    let streaming = move || ctx.state.get().streaming;
    let injection = move || evaluation().injection;
    // 0 待复制，1 已复制，2 复制失败。
    let copy_state = RwSignal::new(0u8);

    let copy_prompt = move |_| {
        let Some(plan) = injection() else { return; };
        if plan.system_prompt.trim().is_empty() {
            return;
        }
        copy_state.set(0);
        let hook = Some(std::rc::Rc::new(move |ok: bool| {
            copy_state.set(if ok { 1 } else { 2 })
        }) as std::rc::Rc<dyn Fn(bool)>);
        crate::io::spawn_copy(plan.system_prompt.clone(), hook);
    };

    // 提示内容变了，复制状态回到「待复制」，避免旧的成功字样留在新提示上。
    Effect::new(move |_| {
        injection();
        copy_state.set(0);
    });

    view! {
        <details class="fold" id="fold-advanced">
            <summary class="fold-summary">{ADVANCED.inject_title}</summary>
            <div class="fold-body">
                <p class="fold-lead">{FLOW.inject_lead}</p>
                <ol class="flow-steps">
                    <li class="flow-step">
                        <span class="flow-step-no">1</span>
                        <div class="flow-step-body">
                            <p class="flow-step-title">{FLOW.inject_steps[0]}</p>
                            <p class="muted small">{FLOW.inject_step1}</p>
                            {move || {
                                injection()
                                    .map(|plan| {
                                        let state = match plan.state {
                                            InjectionState::Injected => "已注入到 system",
                                            InjectionState::Appended => "追加到提示词末尾",
                                            InjectionState::SkippedConflict => "已有语言约定，跳过注入",
                                            InjectionState::Disabled => "本次不注入",
                                        };
                                        let detail = if plan.system_prompt.trim().is_empty() {
                                            plan.reason.clone()
                                        } else {
                                            format!(
                                                "目标语言 {}，提示自身约 {} Token",
                                                lang_text(plan.target_lang),
                                                plan.prompt_tokens.round(),
                                            )
                                        };
                                        view! {
                                            <p class="inject-current" data-testid="inject-current">
                                                <span class="chip">{state}</span>
                                                <span class="muted small">{detail}</span>
                                            </p>
                                        }
                                    })
                            }}
                        </div>
                    </li>

                    <li class="flow-step">
                        <span class="flow-step-no">2</span>
                        <div class="flow-step-body">
                            <p class="flow-step-title">{FLOW.inject_steps[1]}</p>
                            <p class="muted small">{FLOW.inject_step2}</p>
                            {move || {
                                injection()
                                    .map(|plan| {
                                        if plan.system_prompt.trim().is_empty() {
                                            view! {
                                                <p class="notice notice-warn">{plan.reason}</p>
                                            }
                                                .into_any()
                                        } else {
                                            view! {
                                                <div class="inject-copy">
                                                    <code class="code-line">{plan.system_prompt}</code>
                                                    <button
                                                        type="button"
                                                        class="btn btn-sm copy-btn"
                                                        on:click=copy_prompt
                                                    >
                                                        {move || match copy_state.get() {
                                                            1 => FLOW.inject_copied,
                                                            2 => FLOW.inject_copy_fail,
                                                            _ => FLOW.inject_copy,
                                                        }}
                                                    </button>
                                                </div>
                                            }
                                                .into_any()
                                        }
                                    })
                            }}
                            <p class="muted small">{FLOW.inject_stream_note}</p>
                            <p class="muted small">{FLOW.inject_conflict_note}</p>
                        </div>
                    </li>

                    <li class="flow-step">
                        <span class="flow-step-no">3</span>
                        <div class="flow-step-body">
                            <p class="flow-step-title">{FLOW.inject_steps[2]}</p>
                            <p class="muted small">{FLOW.inject_step3}</p>
                            <label class="field-block">
                                <span class="small">{ADVANCED.system_prompt}</span>
                                <textarea
                                    class="prompt-input"
                                    rows="3"
                                    placeholder=ADVANCED.system_prompt_placeholder
                                    prop:value=move || ctx.state.get().user_system_prompt
                                    on:input=move |ev| ctx.set_system_prompt(event_target_value(&ev))
                                ></textarea>
                            </label>
                            <label class="check-row">
                                <input
                                    type="checkbox"
                                    prop:checked=streaming
                                    on:change=move |_| ctx.toggle_streaming()
                                />
                                <span>{ADVANCED.streaming}</span>
                            </label>
                        </div>
                    </li>
                </ol>

                <div class="post-panel">
                    <p class="sub-title">{ADVANCED.post_title}</p>
                    <ul class="post-list">
                        <li>{ADVANCED.post_tags}</li>
                        <li>
                            {move || {
                                if streaming() { ADVANCED.post_polish_off } else { ADVANCED.post_polish_on }
                            }}
                        </li>
                        <li>
                            {move || {
                                ADVANCED
                                    .post_expected(evaluation().routing.output_lang)
                            }}
                        </li>
                        <li>
                            {format!("漂移阈值 {:.0}%：低于该纯度只提示，不阻断回答。", DRIFT_THRESHOLD * 100.0)}
                        </li>
                    </ul>
                </div>

                {move || {
                    (ctx.state.get().calibration.status == CalibrationStatus::Error)
                        .then(|| {
                            view! {
                                <p class="muted small">{CAL.api_retry_hint}</p>
                            }
                        })
                }}
            </div>
        </details>
    }
}
