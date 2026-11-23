//! LangBridge Web 前端，Leptos CSR 渲染。界面编排在 `app`，业务计算在 `langbridge_core`，
//! 浏览器能力封装在 `probe` 与 `io`。

pub mod app;
pub mod copy;
pub mod data;
pub mod io;
pub mod probe;
pub mod state;
pub mod ui;

use wasm_bindgen::prelude::*;

/// WASM 挂载入口。`langbridge_core` 从注入的时钟读取当前时间，启动时先写入浏览器时间。
#[wasm_bindgen(start)]
pub fn start() {
    console_error_panic_hook::set_once();
    // `langbridge_core` 不自行读取时钟；未注入时画像过期判定不可用。
    langbridge_core::set_now_ms(js_sys::Date::now() as i64);
    leptos::mount::mount_to_body(app::App);
}
