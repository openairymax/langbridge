//! 页面编排：首屏、工作台、说明、探测、折叠区与页脚。

use leptos::prelude::*;

use crate::copy::{steps, HERO, STEPS_NOTE, STEPS_TITLE};
use crate::state::{install_app_state, AppCtx};
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
