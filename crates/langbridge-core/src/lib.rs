//! LangBridge 核心引擎：纯 Rust、无平台依赖，可编译到 wasm32 与原生。
//!
//! 分层：
//! - `signals`   信号提取（语言 / 任务 / 规模）
//! - `calibrate` 差分校准与质量闸门
//! - `profiles`  模型元数据与画像
//! - `savings`   净收益与门槛
//! - `router`    四层路由
//! - `inject`    语言约束注入
//! - `guard`     输出后处理
//! - `canonical` 规范化数据与遥测

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

/// 平台层注入的当前时间（Unix 毫秒）
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
    // wasm 目标下由 web crate 在启动时写入时钟；未写入时返回 0（过期判定退化为“可用”）
    CLOCK.with(|c| c.get())
}

#[cfg(target_arch = "wasm32")]
thread_local! {
    static CLOCK: std::cell::Cell<i64> = const { std::cell::Cell::new(0) };
}

/// 由 Web 层在每个请求前同步浏览器时间；原生目标直接用系统时钟，注入为空操作。
pub fn set_now_ms(ms: i64) {
    #[cfg(target_arch = "wasm32")]
    CLOCK.with(|c| c.set(ms));
    #[cfg(not(target_arch = "wasm32"))]
    let _ = ms;
}
