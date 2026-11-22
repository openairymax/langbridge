//! 页面编排：英雄区、工作台（输入 | 结论）、三步说明、API 探测、折叠区与页脚。
//! 只做布局与装配；文案在 `copy`，状态在 `state`，业务在 core。

use leptos::prelude::*;

use crate::copy::{HERO, STEPS_NOTE, STEPS_TITLE, steps};
use crate::state::{AppCtx, install_app_state};
use crate::ui::ask::AskCard;
use crate::ui::folds::Folds;
use crate::ui::probe_card::ApiProbe;
use crate::ui::result::ResultCard;

#[component]
pub fn App() -> impl IntoView {
    let ctx = install_app_state();
    provide_context(ctx);
    view! {
        <div class="app">
            <main class="main">
                <Hero />
                <section class="workbench" aria-label="工作台">
                    <AskCard />
                    <ResultCard />
                </section>
                <StepsStrip />
                <ApiProbe />
                <Folds />
            </main>
            <Footer />
        </div>
    }
}

/// 首屏：品牌名、定位句、成本问句与来源。顶部只剩一条存储告警。
#[component]
fn Hero() -> impl IntoView {
    let ctx = expect_context::<AppCtx>();
    let warning = move || ctx.state.get().storage_warning;
    view! {
        <section class="hero">
            {move || {
                warning().map(|message| {
                    view! {
                        <div class="top-meta">
                            <span class="chip chip-warn">{message}</span>
                        </div>
                    }
                })
            }}
            <h1 class="hero-title">{HERO.app_name}</h1>
            <p class="hero-subtitle">{HERO.title}</p>
            <p class="hero-sub">{HERO.sub}</p>
            <p class="hero-origin">
                "源于 "
                <a href=HERO.origin.url target="_blank" rel="noreferrer">
                    {HERO.origin.org}
                </a>
                {HERO.origin.text}
            </p>
        </section>
    }
}

/// 三步说明：措辞与档位全部来自文案层与样本数据。
#[component]
fn StepsStrip() -> impl IntoView {
    let rows = steps();
    view! {
        <section class="card steps-strip" aria-label=STEPS_TITLE>
            <ol class="strip-list">
                {rows
                    .into_iter()
                    .map(|step| {
                        view! {
                            <li class="strip-item">
                                <span class="strip-no">{step.no}</span>
                                <div>
                                    <strong>{step.title}</strong>
                                    <p class="muted small">{step.text}</p>
                                </div>
                            </li>
                        }
                    })
                    .collect_view()}
            </ol>
            <p class="steps-note">{STEPS_NOTE}</p>
        </section>
    }
}

#[component]
fn Footer() -> impl IntoView {
    view! {
        <footer class="footer">
            <p class="footer-line">{crate::copy::FOOTER.line}</p>
            <p class="footer-note muted small">{crate::copy::FOOTER.note}</p>
            <a class="footer-org" href=crate::copy::FOOTER.org_url target="_blank" rel="noreferrer">
                {crate::copy::FOOTER.org}
            </a>
        </footer>
    }
}
