//! 模型元数据与原生语言推断。原生语言以元数据为准，不用比值反猜已登记模型。

use crate::types::{CalibrationResult, Family, Lang, ModelMeta, ModelProfile, Policy, PolicyMode, Provenance};

/// 死区只用于吸收测量噪声；是否值得切换由净收益门槛决定
pub const RATIO_HIGH_DEFAULT: f64 = 1.10;
pub const RATIO_LOW_DEFAULT: f64 = 0.9;

fn meta(
    model_id: &str,
    display_name: &str,
    family: Family,
    native_lang: Option<Lang>,
    reasoning_capable: bool,
    context_limit: u32,
) -> ModelMeta {
    ModelMeta {
        model_id: model_id.to_string(),
        display_name: display_name.to_string(),
        family,
        native_lang,
        reasoning_capable,
        context_limit,
    }
}

pub fn models() -> Vec<ModelMeta> {
    vec![
        meta(
            "deepseek-reasoner",
            "DeepSeek-R1",
            Family::Deepseek,
            Some(Lang::Zh),
            true,
            65536,
        ),
        meta(
            "deepseek-chat",
            "DeepSeek-V3",
            Family::Deepseek,
            Some(Lang::Zh),
            false,
            65536,
        ),
        meta("qwen3-max", "Qwen3-Max", Family::Qwen, Some(Lang::Zh), true, 262144),
        meta("gpt-4o", "GPT-4o", Family::Other, Some(Lang::En), false, 128000),
        meta(
            "claude-sonnet-4-5",
            "Claude Sonnet 4.5",
            Family::Other,
            Some(Lang::En),
            false,
            200000,
        ),
    ]
}

pub fn list_models() -> Vec<ModelMeta> {
    models()
}

pub fn get_model(model_id: &str) -> Option<ModelMeta> {
    models().into_iter().find(|m| m.model_id == model_id)
}

pub fn family_of(model_id: &str) -> Family {
    let lower = model_id.to_ascii_lowercase();
    if lower.starts_with("qwen") {
        Family::Qwen
    } else if lower.starts_with("deepseek") {
        Family::Deepseek
    } else {
        Family::Other
    }
}

pub fn reasoning_capable(model_id: &str) -> bool {
    if let Some(m) = get_model(model_id) {
        return m.reasoning_capable;
    }
    let lower = model_id.to_ascii_lowercase();
    lower.contains("reasoner")
        || lower.contains("r1")
        || lower.contains("qwq")
        || lower.contains("thinking")
        || regex_o_series(&lower)
}

fn regex_o_series(lower: &str) -> bool {
    // 对齐 /^o[1-9]/
    let mut chars = lower.chars();
    matches!(chars.next(), Some('o')) && matches!(chars.next(), Some(c) if ('1'..='9').contains(&c))
}

/// 原生语言推断：已登记模型读元数据；未知模型返回 None（需校准）
pub fn infer_native_lang(model_id: &str, family: Option<Family>, ratio: Option<f64>) -> Option<Lang> {
    if let Some(known) = get_model(model_id) {
        return known.native_lang;
    }
    let fam = family.unwrap_or_else(|| family_of(model_id));
    if fam == Family::Qwen {
        return Some(Lang::Zh);
    }
    if model_id.to_ascii_lowercase().starts_with("deepseek-v3") {
        return Some(Lang::Zh);
    }
    match fam {
        Family::Deepseek => ratio.map(|r| if r > RATIO_HIGH_DEFAULT { Lang::En } else { Lang::Zh }),
        Family::Other => Some(Lang::En),
        Family::Qwen => Some(Lang::Zh),
    }
}

/// 未登记模型的安全元数据
pub fn meta_for(model_id: &str, ratio: Option<f64>) -> ModelMeta {
    if let Some(known) = get_model(model_id) {
        return known;
    }
    let family = family_of(model_id);
    ModelMeta {
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        family,
        native_lang: infer_native_lang(model_id, Some(family), ratio),
        reasoning_capable: reasoning_capable(model_id),
        context_limit: 65536,
    }
}

/// 由校准结果生成完整画像 v3
pub fn build_profile(model: ModelMeta, measurement: CalibrationResult) -> ModelProfile {
    let native = model
        .native_lang
        .or_else(|| infer_native_lang(&model.model_id, Some(model.family), Some(measurement.effective_ratio)));
    let mut model = model;
    model.native_lang = native;
    let reproduce = format!(
        "cargo run -p langbridge-cli -- calibrate --model {}",
        model.model_id
    );
    let source = measurement.method;
    ModelProfile {
        schema: ModelProfile::SCHEMA.to_string(),
        version: ModelProfile::VERSION,
        model,
        measurement,
        policy: Policy {
            mode: PolicyMode::Balanced,
            switch_threshold_tokens: 50.0,
            switch_threshold_pct: 0.08,
            ratio_high: RATIO_HIGH_DEFAULT,
            ratio_low: RATIO_LOW_DEFAULT,
        },
        provenance: Provenance {
            source,
            reproduce,
            migrated: None,
        },
    }
}

/// 容量越界 fail-fast
pub fn assert_within_capacity(count: u64, capacity: u64) -> Result<(), crate::types::EngineError> {
    if count > capacity {
        return Err(crate::types::EngineError::new(
            crate::types::ErrorCode::InvalidProfile,
            format!("容量越界：{} > {}，整批拒绝（fail-fast）", count, capacity),
        ));
    }
    Ok(())
}
