//! LangBridge Web（Leptos CSR）：界面编排在 `app`，业务在 core，浏览器能力在 `probe` / `io`。

pub mod app;
pub mod copy;
pub mod data;
pub mod io;
pub mod probe;
pub mod state;
pub mod ui;

use wasm_bindgen::prelude::*;

/// 挂载入口：同步浏览器时钟，装好 panic 钩子，再交给 Leptos。
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    // core 在 wasm 下读取注入时钟；不注入会让画像过期判定退化。
    langbridge_core::set_now_ms(js_sys::Date::now() as i64);
    leptos::mount::mount_to_body(app::App);
}
