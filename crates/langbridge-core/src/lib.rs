//! LangBridge 核心引擎。纯 Rust，无平台依赖，可编译到 wasm32 与原生。
//!
//! 覆盖信号提取、校准、画像、净收益、路由、注入与输出后处理。
//! 对平台的依赖仅限时间来源：wasm 由平台层写入，原生读取系统时钟。

pub mod calibrate;
pub mod canonical;
pub mod evidence;
pub mod guard;
pub mod inject;
pub mod profile_store;
pub mod profiles;
pub mod router;
pub mod samples;
pub mod savings;
pub mod signals;
pub mod time;
pub mod types;

pub use types::*;

/// 当前 Unix 毫秒。wasm 目标读取平台层写入的时钟，原生目标读取系统时钟。
pub fn now_ms() -> i64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_now_ms()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
}

#[cfg(target_arch = "wasm32")]
fn js_now_ms() -> i64 {
    // 时钟未写入时返回 0，过期判定因此不会触发
    CLOCK.with(|c| c.get())
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLOCK: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// 由平台层写入当前时间；原生目标忽略该值。
pub fn set_now_ms(ms: i64) {
    #[cfg(target_arch = "wasm32")]
    CLOCK.with(|c| c.set(ms));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = ms;
}
