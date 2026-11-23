//! LangBridge Web 前端，Leptos CSR 渲染。

pub mod app;
pub mod copy;
pub mod data;
pub mod io;
pub mod probe;
pub mod state;
pub mod ui;

use wasm_bindgen::prelude::*;

#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    langbridge_core::set_now_ms(js_sys::Date::now() as i64);
    leptos::mount::mount_to_body(app::App);
}
