//! 浏览器能力封装：localStorage、剪贴板、下载与文件读取。
//! 存储不可用时一律降级为 false / None，由调用方提示并继续运行。

use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;
use web_sys::{Blob, BlobPropertyBag, File, FileReader, HtmlAnchorElement, Url};

pub fn window() -> Option<web_sys::Window> {
    web_sys::window()
}

fn storage() -> Option<web_sys::Storage> {
    window()?.local_storage().ok().flatten()
}

pub fn storage_available() -> bool {
    let Some(s) = storage() else {
        return false;
    };
    let key = "__langbridge_probe__";
    let ok = s.set_item(key, "1").is_ok() && s.get_item(key).ok().flatten().as_deref() == Some("1");
    let _ = s.remove_item(key);
    ok
}

pub fn get_item(key: &str) -> Option<String> {
    storage()?.get_item(key).ok().flatten()
}

pub fn set_item(key: &str, value: &str) -> bool {
    match storage() {
        Some(s) => s.set_item(key, value).is_ok(),
        None => false,
    }
}

pub fn remove_item(key: &str) {
    if let Some(s) = storage() {
        let _ = s.remove_item(key);
    }
}

/// 与 TS `navigator.clipboard.writeText` 对齐；不支持的浏览器返回 false。
pub async fn copy_text(text: &str) -> bool {
    let Some(win) = window() else {
        return false;
    };
    let clipboard = win.navigator().clipboard();
    match JsFuture::from(clipboard.write_text(text)).await {
        Ok(_) => true,
        Err(_) => false,
    }
}

pub fn spawn_copy(text: String, on_done: Option<std::rc::Rc<dyn Fn(bool)>>) {
    wasm_bindgen_futures::spawn_local(async move {
        let ok = copy_text(&text).await;
        if let Some(cb) = on_done {
            cb(ok);
        }
    });
}

/// 跳到指定区块，用于「去 API 测量」这类跨区引导。
pub fn scroll_to(id: &str) {
    let Some(win) = window() else { return; };
    if let Some(el) = win.document().and_then(|doc| doc.get_element_by_id(id)) {
        el.scroll_into_view();
    }
}

/// 单列布局下结论在输入区下方，示例点完把页面带到结论；双列不动，避免桌面端抢滚动。
pub fn reveal_verdict_if_stacked() {
    let Some(win) = window() else {
        return;
    };
    let narrow = win
        .inner_width()
        .ok()
        .and_then(|value| value.as_f64())
        .map(|width| width < 981.0)
        .unwrap_or(false);
    if !narrow {
        return;
    }
    if let Some(el) = win.document().and_then(|doc| doc.get_element_by_id("verdict")) {
        el.scroll_into_view();
    }
}

/// 与 TS `downloadText` 对齐：Blob + 临时 a 标签，1 秒后释放 object URL。
pub fn download_text(filename: &str, text: &str, mime: &str) {
    let Some(win) = window() else {
        return;
    };
    let Some(doc) = win.document() else {
        return;
    };
    let parts = js_sys::Array::new();
    parts.push(&JsValue::from_str(text));
    let bag = BlobPropertyBag::new();
    bag.set_type(&format!("{};charset=utf-8", mime));
    let Ok(blob) = Blob::new_with_str_sequence_and_options(&parts, &bag) else {
        return;
    };
    let Ok(url) = Url::create_object_url_with_blob(&blob) else {
        return;
    };
    let Some(el) = doc.create_element("a").ok() else {
        let _ = Url::revoke_object_url(&url);
        return;
    };
    let Ok(a) = el.dyn_into::<HtmlAnchorElement>() else {
        let _ = Url::revoke_object_url(&url);
        return;
    };
    a.set_href(&url);
    a.set_download(filename);
    if let Some(body) = doc.body() {
        let _ = body.append_child(&a);
    }
    a.click();
    if let Some(parent) = a.parent_node() {
        let _ = parent.remove_child(&a);
    }
    let revoke_url = url.clone();
    gloo_timers::callback::Timeout::new(1000, move || {
        let _ = Url::revoke_object_url(&revoke_url);
    })
    .forget();
}

/// 与 TS `readFileAsText(file)` 对齐：UTF-8 读取失败时 reject。
pub async fn read_file_text(file: &File) -> Result<String, String> {
    let promise = js_sys::Promise::new(&mut {
        let file = file.clone();
        move |resolve, reject| {
            let Ok(reader) = FileReader::new() else {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("读取文件失败"));
                return;
            };
            let success_reader = reader.clone();
            let onload = Closure::once_into_js(move || {
                let text = success_reader
                    .result()
                    .ok()
                    .and_then(|v| v.as_string())
                    .unwrap_or_default();
                let _ = resolve.call1(&JsValue::NULL, &JsValue::from_str(&text));
            });
            reader.set_onload(Some(onload.unchecked_ref()));
            let onerror = Closure::once_into_js(move || {
                let _ = reject.call1(&JsValue::NULL, &JsValue::from_str("读取文件失败"));
            });
            reader.set_onerror(Some(onerror.unchecked_ref()));
            let _ = reader.read_as_text(&file);
        }
    });
    match JsFuture::from(promise).await {
        Ok(value) => Ok(value.as_string().unwrap_or_default()),
        Err(err) => Err(err.as_string().unwrap_or_else(|| "读取文件失败".to_string())),
    }
}
