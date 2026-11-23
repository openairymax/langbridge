//! 结论卡：结论、数字、复制与下载，以及判断过程。

use leptos::prelude::*;

use crate::copy::{
    applicability, builtin_row_text, chain_foot, layer, prompt_suffix, r_preset_aria,
    result_net_text_for, result_ratio_text, result_summary_text, save_line_parts, savings_story,
    signal_rows, verdict_copy, why_mark, SavingsStory, CHAIN, EMPTY, FLOW, GATE_NET,
    GATE_THRESHOLD, RESULT, R_LIVE_NOTE, R_PRESETS, R_ROW_LABEL, SUFFIX, UI,
};
use crate::data::builtin_rows;
use crate::state::AppCtx;

#[component]
pub fn ResultCard() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let has_text = move || !ctx.state.get().text.trim().is_empty();

    view! {
        <section class="card result" id="verdict" data-testid="result-card">
            {move || {
                if has_text() {
                    view! { <FilledResult /> }.into_any()
                } else {
                    view! { <EmptyState /> }.into_any()
                }
            }}
        </section>
    }
}

#[component]
fn EmptyState() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let rows = builtin_rows();
    let row_count = rows.len();

    view! {
        <div class="empty" data-testid="empty-state">
            <p class="verdict-kicker">{EMPTY.kicker}</p>
            <p class="empty-hint">{EMPTY.hint}</p>
            <div class="empty-builtin">
                <div class="empty-builtin-head">
                    <span class="small muted">{EMPTY.builtin_title}</span>
                    <span class="chip">{EMPTY.builtin_count(row_count)}</span>
                </div>
                <p class="builtin-pick">{FLOW.empty_pick}</p>
                <ul class="builtin-list">
                    {rows
                        .iter()
                        .map(|row| {
                            let name = row.name.clone();
                            let detail = builtin_row_text(row.ratio);
                            let id = row.id.clone();
                            view! {
                                <li>
                                    <button
                                        type="button"
                                        class="builtin-row"
                                        data-testid="builtin-row"
                                        on:click=move |_| {
                                            ctx.load_model_demo(id.clone());
                                            crate::io::reveal_verdict_if_stacked();
                                        }
                                    >
                                        <span>{name}</span>
                                        <span class="muted small">{detail}</span>
                                    </button>
                                </li>
                            }
                        })
                        .collect_view()}
                </ul>
                <p class="muted small">{EMPTY.builtin_hint()}</p>
            </div>
            <ul class="empty-points">
                {EMPTY.items.iter().map(|item| view! { <li>{*item}</li> }).collect_view()}
            </ul>
        </div>
    }
}

fn fmt_num(value: f64) -> String {
    if value.is_finite() && value.fract() == 0.0 {
        format!("{:.0}", value)
    } else {
        format!("{}", value)
    }
}

#[component]
fn FilledResult() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let evaluation = move || ctx.evaluation();
    let verdict = move || verdict_copy(&evaluation());
    let story = move || savings_story(&evaluation());
    let badge = move || applicability(&evaluation());
    let suffix = move || prompt_suffix(&evaluation().routing);
    let copied = RwSignal::new(0u8);
    let copied_suffix = RwSignal::new(0u8);
    let download_state = RwSignal::new(0u8);

    let copy_all = {
        move |_| {
            let evaluation = ctx.evaluation();
            let generated = langbridge_core::time::iso_from_ms(langbridge_core::now_ms());
            let text = result_summary_text(&evaluation, &generated);
            let copied = copied;
            crate::io::spawn_copy(
                text,
                Some(std::rc::Rc::new(move |ok: bool| {
                    copied.set(if ok { 1 } else { 2 });
                    set_timeout(move || copied.set(0), if ok { 1600 } else { 2600 });
                })),
            );
        }
    };

    let download = move |_| {
        let evaluation = ctx.evaluation();
        let generated = langbridge_core::time::iso_from_ms(langbridge_core::now_ms());
        let text = result_summary_text(&evaluation, &generated);
        let ok = crate::io::download_text("langbridge-verdict.md", &text, "text/markdown");
        download_state.set(if ok { 1 } else { 2 });
        set_timeout(move || download_state.set(0), if ok { 1600 } else { 2600 });
    };

    view! {
        <>
            <div class="verdict" aria-live="polite">
                <div class="verdict-head">
                    <p class="verdict-kicker">{move || verdict().kicker}</p>
                    <span class=move || format!("chip chip-{}", badge().tone.as_str())>
                        {move || badge().label}
                    </span>
                </div>
                <h2 class="verdict-headline">{move || verdict().headline}</h2>
                <p class="verdict-detail">{move || verdict().detail}</p>
            </div>

            {move || {
                let story = story();
                match &story {
                    SavingsStory::Save { net, .. } => {
                        let (lead, tail) = save_line_parts(&story)
                            .map(|parts| (parts.lead, parts.tail))
                            .unwrap_or_default();
                        let net = fmt_num(*net);
                        view! {
                            <p class="save-line" data-testid="save-line">
                                {lead}
                                <strong>{net}</strong>
                                {tail}
                            </p>
                        }
                            .into_any()
                    }
                    SavingsStory::Lose { .. } => ().into_any(),
                    SavingsStory::Below { .. } => ().into_any(),
                    SavingsStory::Same { .. } => {
                        view! { <p class="note-line same" data-testid="save-line">{story.text()}</p> }
                            .into_any()
                    }
                    SavingsStory::Blocked { .. } => {
                        view! { <p class="note-line blocked" data-testid="save-line">{story.text()}</p> }
                            .into_any()
                    }
                    SavingsStory::Quality { .. } => {
                        view! { <p class="note-line quality" data-testid="save-line">{story.text()}</p> }
                            .into_any()
                    }
                    SavingsStory::Na { .. } => {
                        view! { <p class="note-line na" data-testid="save-line">{story.text()}</p> }
                            .into_any()
                    }
                }
            }}

            <p class="result-meta" data-testid="result-meta">
                <span>{move || result_ratio_text(evaluation().effective_ratio)}</span>
                <span class="result-meta-sep" aria-hidden="true">"/"</span>
                <span>{move || result_net_text_for(&ctx.evaluation())}</span>
            </p>

            {move || {
                ctx.evaluation()
                    .model
                    .reasoning_capable
                    .then(|| view! { <LiveGate /> })
            }}

            <div class="result-actions">
                <button type="button" class="btn btn-sm copy-btn" on:click=copy_all>
                    {move || match copied.get() {
                        1 => RESULT.copied,
                        2 => RESULT.copy_failed,
                        _ => RESULT.copy,
                    }}
                </button>
                <button type="button" class="btn btn-sm btn-ghost" on:click=download>
                    {move || match download_state.get() {
                        1 => RESULT.download_started,
                        2 => RESULT.download_failed,
                        _ => RESULT.download,
                    }}
                </button>
            </div>

            {move || {
                let injectable = ctx
                    .evaluation()
                    .injection
                    .as_ref()
                    .map(|plan| !plan.system_prompt.trim().is_empty())
                    .unwrap_or(false);
                injectable
                    .then(|| {
                        view! {
                            <div class="suffix-box" data-testid="prompt-suffix">
                                <div class="suffix-head">
                                    <span class="suffix-label">{SUFFIX.label}</span>
                                    <button
                                        type="button"
                                        class="btn btn-ghost btn-sm copy-btn"
                                        on:click=move |_| {
                                            let text = suffix();
                                            let copied_suffix = copied_suffix;
                                            crate::io::spawn_copy(
                                                text,
                                                Some(std::rc::Rc::new(move |ok: bool| {
                                                    copied_suffix.set(if ok { 1 } else { 2 });
                                                    set_timeout(
                                                        move || copied_suffix.set(0),
                                                        if ok { 1600 } else { 2600 },
                                                    );
                                                })),
                                            );
                                        }
                                    >
                                        {move || match copied_suffix.get() {
                                            1 => UI.copied,
                                            2 => UI.copy_failed,
                                            _ => SUFFIX.copy,
                                        }}
                                    </button>
                                </div>
                                <code class="suffix-text">{move || suffix()}</code>
                                <p class="muted small">{SUFFIX.hint}</p>
                            </div>
                        }
                    })
            }}

            <WhyThis />
        </>
    }
}

#[component]
fn LiveGate() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let r_now = move || ctx.evaluation().r;
    let manual = move || ctx.state.get().r_override;

    let chip_class = move |value: Option<f64>| {
        if manual() == value {
            "r-gate-chip on"
        } else {
            "r-gate-chip"
        }
    };

    view! {
        <div class="r-gate" data-testid="live-gate">
            <div class="r-gate-head">
                <span class="r-gate-label" title=R_LIVE_NOTE>
                    {R_ROW_LABEL}
                    <strong>{move || fmt_num(r_now())}</strong>
                </span>
                <div class="r-gate-chips" role="group" aria-label=R_ROW_LABEL>
                    <button
                        type="button"
                        class=move || chip_class(None)
                        aria-pressed=move || manual().is_none().to_string()
                        on:click=move |_| ctx.set_r_override(None)
                    >
                        "自动"
                    </button>
                    {R_PRESETS
                        .iter()
                        .map(|value| {
                            let value = *value;
                            view! {
                                <button
                                    type="button"
                                    class=move || chip_class(Some(value))
                                    aria-label=r_preset_aria(value)
                                    aria-pressed=move || (manual() == Some(value)).to_string()
                                    on:click=move |_| ctx.set_r_override(Some(value))
                                >
                                    {fmt_num(value)}
                                </button>
                            }
                        })
                        .collect_view()}
                </div>
            </div>

            {move || {
                ctx.evaluation()
                    .savings
                    .clone()
                    .map(|savings| {
                        let max = savings.net.abs().max(savings.threshold).max(1.0);
                        let net_width = (savings.net / max * 100.0).clamp(0.0, 100.0);
                        let gate_width = (savings.threshold / max * 100.0).clamp(0.0, 100.0);
                        view! {
                            <div class="r-gate-rows">
                                <div class="r-gate-row">
                                    <span class="r-gate-name">{GATE_NET}</span>
                                    <div class="r-gate-track">
                                        <span
                                            class="r-gate-fill"
                                            style=format!("width: {:.1}%", net_width)
                                        ></span>
                                    </div>
                                    <span class="r-gate-value">{fmt_num(savings.net)}</span>
                                </div>
                                <div class="r-gate-row">
                                    <span class="r-gate-name">{GATE_THRESHOLD}</span>
                                    <div class="r-gate-track">
                                        <span
                                            class="r-gate-fill threshold"
                                            style=format!("width: {:.1}%", gate_width)
                                        ></span>
                                    </div>
                                    <span class="r-gate-value">{fmt_num(savings.threshold)}</span>
                                </div>
                            </div>
                            <p class="r-gate-note small muted">
                                {format!(
                                    "毛收益 {}，注入开销 {}",
                                    fmt_num(savings.gross),
                                    fmt_num(savings.overhead),
                                )}
                            </p>
                        }
                            .into_any()
                    })
            }}
        </div>
    }
}

#[component]
fn WhyThis() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let evaluation = move || ctx.evaluation();
    let rows = move || {
        let evaluation = evaluation();
        signal_rows(&evaluation.signals, evaluation.signals.input_chars)
    };
    view! {
        <div class="why" data-testid="why-this">
            <p class="sub-title">{CHAIN.title}</p>
            <ol class="chain">
                {crate::copy::LAYER_ORDER
                    .iter()
                    .map(|layer_id| {
                        let copy = layer(*layer_id);
                        let name = copy.name;
                        let rule = copy.rule;
                        view! {
                            <li class=move || {
                                let ev = evaluation();
                                let step = ev.routing.steps.iter().find(|s| s.layer == *layer_id);
                                let state = match step {
                                    Some(s) if s.hit => "hit",
                                    Some(s) if s.terminated => "stop",
                                    _ => "idle",
                                };
                                format!("chain-row {}", state)
                            }>
                                <span class="chain-name">{name}</span>
                                <span class="chain-rule">{rule}</span>
                                <span class="chain-mark">
                                    {move || {
                                        let evaluation = evaluation();
                                        let step = evaluation.routing.steps.iter().find(|s| s.layer == *layer_id);
                                        why_mark(step)
                                    }}
                                </span>
                            </li>
                        }
                    })
                    .collect_view()}
            </ol>
            <p class="chain-foot">{move || chain_foot(&evaluation().routing)}</p>

            <div class="signal-panel">
                {move || {
                    rows()
                        .into_iter()
                        .map(|row| {
                            view! {
                                <div class="signal-cell">
                                    <span class="signal-label">{row.label}</span>
                                    <span class="signal-value">{row.value}</span>
                                    {row
                                        .sub
                                        .map(|sub| view! { <span class="signal-sub muted small">{sub}</span> })}
                                </div>
                            }
                        })
                        .collect_view()
                }}
            </div>
        </div>
    }
}

fn set_timeout<F: FnOnce() + 'static>(f: F, after: u32) {
    gloo_timers::callback::Timeout::new(after, f).forget();
}
