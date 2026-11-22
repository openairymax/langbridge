//! 输入卡：选模型、看当前画像、写问题、一键示例。
//! 交互取舍：模型切换立即生效；自定义模型在失焦或回车时提交，避免每敲一个字就重算。
//! 校准按钮的文案与动作由画像来源决定，不额外堆按钮。

use leptos::prelude::*;
use wasm_bindgen::JsCast;

use langbridge_core::evidence::has_evidence;
use langbridge_core::profiles::models;

use crate::copy::{DEMO, ModelNoteKind, UI, action_label, fmt2, model_note};
use crate::data::EXAMPLES;
use crate::state::{AppCtx, CalibrationStatus, source_key};

const CUSTOM_ID: &str = "__custom__";

fn note_kind_class(kind: ModelNoteKind) -> &'static str {
    match kind {
        ModelNoteKind::Builtin => "builtin",
        ModelNoteKind::Measured => "measured",
        ModelNoteKind::Stale => "stale",
        ModelNoteKind::LowQuality => "low",
        ModelNoteKind::None => "none",
    }
}

#[component]
pub fn AskCard() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let known = models();
    let current_model = move || ctx.state.get().model_id;
    let measured = move || ctx.state.get().measured;
    let is_known = |id: &str| models().iter().any(|m| m.model_id == id);
    let initially_listed = is_known(&ctx.state.get_untracked().model_id)
        || ctx
            .state
            .get_untracked()
            .measured
            .iter()
            .any(|m| m.id == ctx.state.get_untracked().model_id);
    let custom_mode = RwSignal::new(!initially_listed);
    let custom_text = RwSignal::new(current_model());

    // 测量完成后，模型已经出现在下拉里，自动收起手输框，避免两个入口同时亮着。
    let measured_len = RwSignal::new(measured().len());
    Effect::new(move |_| {
        let list = measured();
        if list.len() == measured_len.get_untracked() {
            return;
        }
        measured_len.set(list.len());
        let id = ctx.state.get_untracked().model_id;
        let typed = custom_text.get_untracked();
        if list.iter().any(|m| m.id == id || m.id == typed) {
            custom_mode.set(false);
        }
    });

    // 自定义模型提交：只在真正变化时触发一次重算。
    let commit_custom = move || {
        let value = custom_text.get_untracked().trim().to_string();
        if value.is_empty() || value == ctx.state.get_untracked().model_id {
            return;
        }
        ctx.set_model(value);
    };

    let on_select = move |ev: leptos::ev::Event| {
        let value = ev
            .target()
            .and_then(|t| t.dyn_into::<web_sys::HtmlSelectElement>().ok())
            .map(|el| el.value())
            .unwrap_or_default();
        if value == CUSTOM_ID {
            custom_mode.set(true);
            custom_text.set(ctx.state.get_untracked().model_id);
            return;
        }
        custom_mode.set(false);
        if value != ctx.state.get_untracked().model_id {
            ctx.set_model(value);
        }
    };

    let note = move || model_note(&ctx.evaluation());
    let running = move || ctx.state.get().calibration.status == CalibrationStatus::Running;

    let calibrate = move |_| {
        let id = ctx.state.get_untracked().model_id;
        ctx.calibrate(Some(id));
    };

    // 一键示例放在输入区，首屏就能点到；点完在单列布局下自动带到结论。
    let run_demo = move |_| {
        ctx.load_demo();
        crate::io::reveal_verdict_if_stacked();
    };

    view! {
        <section class="card ask" aria-label=UI.ask_aria data-testid="ask-card">
            <div class="model-row">
                <span class="model-label">{UI.model_label}</span>
                <select
                    class="model-select"
                    aria-label=UI.model_select_aria
                    on:change=on_select
                    prop:value=move || {
                        if custom_mode.get() { CUSTOM_ID.to_string() } else { current_model() }
                    }
                >
                    {known
                        .iter()
                        .map(|item| {
                            let id = item.model_id.clone();
                            let lookup = id.clone();
                            let name = item.display_name.clone();
                            view! {
                                <option value=id>
                                    {move || {
                                        measured()
                                            .iter()
                                            .find(|m| m.id == lookup)
                                            .map(|m| format!("{} · 实测 {}×", name, fmt2(m.ratio)))
                                            .unwrap_or_else(|| name.clone())
                                    }}
                                </option>
                            }
                        })
                        .collect_view()}
                    {move || {
                        let extras: Vec<_> = measured()
                            .into_iter()
                            .filter(|m| !is_known(&m.id))
                            .collect();
                        (!extras.is_empty())
                            .then(|| {
                                view! {
                                    <optgroup label=UI.measured_group>
                                        {extras
                                            .into_iter()
                                            .map(|m| {
                                                view! {
                                                    <option value=m.id.clone()>
                                                        {format!("{} · {}×", m.name, fmt2(m.ratio))}
                                                    </option>
                                                }
                                            })
                                            .collect_view()}
                                    </optgroup>
                                }
                            })
                    }}
                    <option value=CUSTOM_ID>{UI.other_model}</option>
                </select>
                {move || {
                    custom_mode
                        .get()
                        .then(|| {
                            view! {
                                <input
                                    class="model-custom"
                                    aria-label=UI.model_custom_aria
                                    placeholder=UI.model_custom_placeholder
                                    prop:value=move || custom_text.get()
                                    on:input=move |ev| {
                                        custom_text
                                            .set(event_target_value(&ev));
                                    }
                                    on:blur=move |_| commit_custom()
                                    on:keydown=move |ev| {
                                        if ev.key() == "Enter" {
                                            commit_custom();
                                        }
                                    }
                                />
                            }
                        })
                }}
            </div>

            <div class=move || format!("ask-foot {}", note_kind_class(note().kind)) data-testid="model-note">
                <p class="foot-note">
                    {move || note().lead}
                    {move || {
                        note()
                            .multiple
                            .map(|value| {
                                view! { <strong>{format!("{:.2}", value)}</strong> }
                            })
                    }}
                    {move || note().tail}
                </p>
                <button
                    type="button"
                    class="btn btn-sm btn-ghost"
                    on:click=calibrate
                    disabled=running
                >
                    {move || {
                        let evaluation = ctx.evaluation();
                        let profile = evaluation.profile.as_ref();
                        action_label(
                            &crate::copy::ActionLabelOpts {
                                running: running(),
                                kind: note().kind,
                                method: profile.map(|p| source_key(p.measurement.method)),
                                has_evidence: has_evidence(&evaluation.model.model_id),
                            },
                        )
                    }}
                </button>
            </div>

            <label class="sr-only" for="input-text">
                {UI.input_aria}
            </label>
            <textarea
                id="input-text"
                class="input-text"
                rows="6"
                spellcheck="false"
                placeholder=UI.input_placeholder(EXAMPLES[0].text)
                prop:value=move || ctx.state.get().text
                on:input=move |ev| ctx.set_text(event_target_value(&ev))
            ></textarea>

            {move || {
                ctx.state
                    .get()
                    .text
                    .trim()
                    .is_empty()
                    .then(|| {
                        view! {
                            <div class="demo-entry" data-testid="demo-entry">
                                <button type="button" class="btn btn-primary btn-sm" on:click=run_demo>
                                    {DEMO.button}
                                </button>
                                <span class="small muted">{DEMO.hint}</span>
                            </div>
                        }
                    })
            }}

            <div class="example-row" role="group" aria-label=UI.try_group_aria>
                <span class="model-label">{UI.try_label}</span>
                {EXAMPLES
                    .iter()
                    .map(|ex| {
                        let text = ex.text;
                        let label = ex.label;
                        let hint = ex.hint;
                        view! {
                            <button
                                type="button"
                                class=move || {
                                    let active = ctx.state.get().text == text;
                                    if active { "example example-active" } else { "example" }
                                }
                                title=hint
                                on:click=move |_| {
                                    ctx.set_text(text.to_string());
                                    crate::io::reveal_verdict_if_stacked();
                                }
                            >
                                {label}
                            </button>
                        }
                    })
                    .collect_view()}
                <span class="char-count" data-testid="char-count" aria-live="polite">
                    {move || UI.char_count(ctx.state.get().text.chars().count())}
                </span>
                {move || {
                    (!ctx.state.get().text.is_empty())
                        .then(|| {
                            view! {
                                <button
                                    type="button"
                                    class="clear-input"
                                    on:click=move |_| ctx.set_text(String::new())
                                >
                                    {UI.clear_input}
                                </button>
                            }
                        })
                }}
            </div>
        </section>
    }
}
