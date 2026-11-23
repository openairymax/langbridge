//! 画像的纯逻辑部分：过期判定、内置数据检查与由校准重建画像。
//! 存储由调用方实现。

use serde_json::Value;

use crate::calibrate::{compute_calibration, CalibrationMeta};
use crate::profiles::{build_profile, family_of, get_model, infer_native_lang, reasoning_capable};
use crate::time::parse_iso_ms;
use crate::types::{
    CalibrationResult, FieldError, ModelProfile, Observation, Policy, PolicyMode, ProfileSource,
    Provenance, TokenizerMeta, Warning,
};

pub const PROFILE_SCHEMA: &str = "langbridge.profile";
pub const PROFILE_VERSION: u32 = 3;
pub const PROFILE_KEY: &str = "langbridge.profile.v3";
pub const PROFILE_MAP_KEY: &str = "langbridge.profile.v3.map";
pub const RECENT_KEY: &str = "langbridge.recent.v3";
pub const PROFILE_MAX_AGE_DAYS: i64 = 90;

/// 内置数据来自公开 tokenizer 的真实计数，不随时间失效。
pub fn is_expired(profile: &ModelProfile, now_ms: i64) -> bool {
    if profile.measurement.method == ProfileSource::Builtin {
        return false;
    }
    match parse_iso_ms(&profile.measurement.measured_at) {
        Some(t) => now_ms - t > PROFILE_MAX_AGE_DAYS * 24 * 3600 * 1000,
        None => true,
    }
}

/// 内置参考数据的陈旧提示，仅用于展示，不参与可用性判断。
pub fn is_builtin_stale(profile: &ModelProfile, days: i64, now_ms: i64) -> bool {
    if profile.measurement.method != ProfileSource::Builtin {
        return false;
    }
    match parse_iso_ms(&profile.measurement.measured_at) {
        Some(t) => now_ms - t > days * 24 * 3600 * 1000,
        None => true,
    }
}

pub fn age_days(profile: &ModelProfile, now_ms: i64) -> f64 {
    match parse_iso_ms(&profile.measurement.measured_at) {
        Some(t) => ((now_ms - t) as f64 / (24.0 * 3600.0 * 1000.0)).floor(),
        None => f64::INFINITY,
    }
}

/// 由已有校准结果重建画像。
pub fn profile_from_calibration(
    model_id: &str,
    calibration: CalibrationResult,
    source: Option<ProfileSource>,
) -> ModelProfile {
    let model = get_model(model_id).unwrap_or_else(|| crate::types::ModelMeta {
        model_id: model_id.to_string(),
        display_name: model_id.to_string(),
        family: family_of(model_id),
        native_lang: infer_native_lang(model_id, Some(family_of(model_id)), Some(calibration.effective_ratio)),
        reasoning_capable: reasoning_capable(model_id),
        context_limit: 65536,
    });
    let mut profile = build_profile(model, calibration);
    if let Some(source) = source {
        profile.provenance.source = source;
    }
    profile
}

/* 导入 / 导出 */

#[derive(Debug, Clone, Default)]
pub struct ParseProfileResult {
    pub ok: bool,
    pub errors: Vec<FieldError>,
    pub profile: Option<ModelProfile>,
    pub migrated: bool,
}

impl ParseProfileResult {
    fn fail(errors: Vec<FieldError>) -> Self {
        Self {
            ok: false,
            errors,
            profile: None,
            migrated: false,
        }
    }
}

fn err(path: &str, message: impl Into<String>) -> FieldError {
    FieldError {
        path: path.to_string(),
        message: message.into(),
    }
}

pub fn serialize_profile(profile: &ModelProfile) -> String {
    let mut text = serde_json::to_string_pretty(profile).unwrap_or_default();
    text.push('\n');
    text
}

pub fn profile_file_name(profile: &ModelProfile) -> String {
    format!("langbridge.profile.{}.json", profile.model.model_id)
}

fn num(v: Option<&Value>) -> Option<f64> {
    v.and_then(Value::as_f64).filter(|n| n.is_finite())
}

fn is_lang(v: Option<&Value>) -> bool {
    matches!(v.and_then(Value::as_str), Some("zh") | Some("en"))
}

/// 校验 v3 画像，错误消息可直接展示给用户。
pub fn validate_profile(value: &Value, now_ms: i64) -> ParseProfileResult {
    let Some(p) = value.as_object() else {
        return ParseProfileResult::fail(vec![err("$", "不是合法对象")]);
    };
    let mut errors: Vec<FieldError> = Vec::new();

    if p.get("schema").and_then(Value::as_str) != Some(PROFILE_SCHEMA) {
        errors.push(err("schema", format!("必须为 \"{}\"", PROFILE_SCHEMA)));
    }
    if p.get("version").and_then(Value::as_u64) != Some(PROFILE_VERSION as u64) {
        errors.push(err("version", format!("必须为 {}", PROFILE_VERSION)));
    }

    match p.get("model").and_then(Value::as_object) {
        None => errors.push(err("model", "缺少 model")),
        Some(model) => {
            // 允许 Azure 的斜杠与 Ollama 的冒号，首字符必须为字母或数字
            let model_id = model.get("model_id").and_then(Value::as_str).map(str::trim);
            let id_ok = model_id
                .map(|id| {
                    let mut chars = id.chars();
                    let head_ok = chars
                        .next()
                        .map(|c| c.is_ascii_alphanumeric())
                        .unwrap_or(false);
                    head_ok
                        && id.len() <= 128
                        && id
                            .chars()
                            .all(|c| c.is_ascii_alphanumeric() || "._:@+/-".contains(c))
                })
                .unwrap_or(false);
            if !id_ok {
                errors.push(err(
                    "model.model_id",
                    "必须为 1..128 位合法模型 ID（字母/数字开头）",
                ));
            }
            match model.get("display_name").and_then(Value::as_str) {
                None | Some("") => errors.push(err("model.display_name", "必须为非空字符串")),
                Some(name) if name.chars().count() > 120 => {
                    errors.push(err("model.display_name", "长度不得超过 120"))
                }
                _ => {}
            }
            if !matches!(
                model.get("family").and_then(Value::as_str),
                Some("qwen") | Some("deepseek") | Some("other")
            ) {
                errors.push(err("model.family", "必须为 qwen/deepseek/other"));
            }
            if !is_lang(model.get("native_lang")) {
                errors.push(err("model.native_lang", "必须为 zh/en"));
            }
            if !model
                .get("reasoning_capable")
                .map(Value::is_boolean)
                .unwrap_or(false)
            {
                errors.push(err("model.reasoning_capable", "必须为布尔值"));
            }
            match num(model.get("context_limit")) {
                Some(n) if n > 0.0 => {}
                _ => errors.push(err("model.context_limit", "必须为正数")),
            }
        }
    }

    match p.get("measurement").and_then(Value::as_object) {
        None => errors.push(err("measurement", "缺少 measurement")),
        Some(m) => {
            for key in ["density_ratio", "effective_ratio"] {
                match num(m.get(key)) {
                    Some(n) if (0.5..=2.5).contains(&n) => {}
                    _ => errors.push(err(
                        &format!("measurement.{}", key),
                        "必须在 [0.5, 2.5] 内",
                    )),
                }
            }
            match num(m.get("r2")) {
                Some(n) if (0.0..=1.0).contains(&n) => {}
                _ => errors.push(err("measurement.r2", "必须在 [0, 1] 内")),
            }
            match num(m.get("confidence")) {
                Some(n) if (0.0..=1.0).contains(&n) => {}
                _ => errors.push(err("measurement.confidence", "必须在 [0, 1] 内")),
            }
            let expected = num(m.get("generation_ratio")).or_else(|| num(m.get("density_ratio")));
            if let (Some(eff), Some(exp)) = (num(m.get("effective_ratio")), expected) {
                if (eff - exp).abs() > 0.005 {
                    errors.push(err(
                        "measurement.effective_ratio",
                        "与 generation_ratio/density_ratio 不一致",
                    ));
                }
            }
            if !m
                .get("linearity_ok")
                .map(Value::is_boolean)
                .unwrap_or(false)
            {
                errors.push(err("measurement.linearity_ok", "必须为布尔值"));
            }
            match m.get("measured_at").and_then(Value::as_str) {
                None => errors.push(err("measurement.measured_at", "必须为可解析的时间字符串")),
                Some(raw) => match parse_iso_ms(raw) {
                    None => errors.push(err(
                        "measurement.measured_at",
                        "必须为可解析的时间字符串",
                    )),
                    Some(t) if t > now_ms + 24 * 3600 * 1000 => errors.push(err(
                        "measurement.measured_at",
                        "不得晚于当前时间 24 小时以上",
                    )),
                    _ => {}
                },
            }
            match m.get("observations").and_then(Value::as_array) {
                None => errors.push(err(
                    "measurement.observations",
                    "必须为 6 条观测（每语言 3 档）",
                )),
                Some(obs) => {
                    if obs.len() != 6 {
                        errors.push(err(
                            "measurement.observations",
                            "必须为 6 条观测（每语言 3 档）",
                        ));
                    }
                    let valid = obs.iter().all(|o| {
                        let Some(item) = o.as_object() else {
                            return false;
                        };
                        num(item.get("n")).map(|n| n > 0.0).unwrap_or(false)
                            && num(item.get("prompt_tokens"))
                                .map(|n| n >= 0.0)
                                .unwrap_or(false)
                            && is_lang(item.get("lang"))
                    });
                    if !valid {
                        errors.push(err(
                            "measurement.observations",
                            "每条观测需含合法的 lang/n/prompt_tokens",
                        ));
                    }
                    let zh = obs
                        .iter()
                        .filter(|o| o.get("lang").and_then(Value::as_str) == Some("zh"))
                        .count();
                    let en = obs
                        .iter()
                        .filter(|o| o.get("lang").and_then(Value::as_str) == Some("en"))
                        .count();
                    if zh != 3 || en != 3 {
                        errors.push(err(
                            "measurement.observations",
                            "中文与英文各需 3 条观测",
                        ));
                    }
                }
            }
            match m.get("tokenizer").and_then(Value::as_object) {
                None => errors.push(err("measurement.tokenizer", "缺少 tokenizer")),
                Some(tk) => {
                    let ok = ["name", "version", "sha256"].iter().all(|k| {
                        tk.get(*k)
                            .map(|v| v.is_string())
                            .unwrap_or(false)
                    });
                    if !ok {
                        errors.push(err(
                            "measurement.tokenizer",
                            "tokenizer.name/version/sha256 必须为字符串",
                        ));
                    }
                }
            }
        }
    }

    match p.get("policy").and_then(Value::as_object) {
        None => errors.push(err("policy", "缺少 policy")),
        Some(policy) => {
            if !matches!(
                policy.get("mode").and_then(Value::as_str),
                Some("balanced") | Some("quality")
            ) {
                errors.push(err("policy.mode", "必须为 balanced/quality"));
            }
            match (num(policy.get("ratio_high")), num(policy.get("ratio_low"))) {
                (Some(high), Some(low)) if high > low => {}
                (Some(_), Some(_)) => {
                    errors.push(err("policy.ratio_high", "必须大于 ratio_low"))
                }
                _ => errors.push(err("policy.ratio_high", "必须为数字")),
            }
            match num(policy.get("switch_threshold_tokens")) {
                Some(n) if n >= 0.0 => {}
                _ => errors.push(err(
                    "policy.switch_threshold_tokens",
                    "必须为非负数",
                )),
            }
            match num(policy.get("switch_threshold_pct")) {
                Some(n) if n >= 0.0 => {}
                _ => errors.push(err("policy.switch_threshold_pct", "必须为非负数")),
            }
        }
    }

    match p.get("provenance").and_then(Value::as_object) {
        None => errors.push(err("provenance", "缺少 provenance")),
        Some(prov) => {
            if !matches!(
                prov.get("source").and_then(Value::as_str),
                Some("builtin") | Some("api_probe") | Some("live_run") | Some("manual")
            ) {
                errors.push(err(
                    "provenance.source",
                    "来源必须为 builtin/api_probe/live_run/manual",
                ));
            }
        }
    }

    if !errors.is_empty() {
        return ParseProfileResult::fail(errors);
    }
    match serde_json::from_value::<ModelProfile>(value.clone()) {
        Ok(profile) => ParseProfileResult {
            ok: true,
            errors: Vec::new(),
            profile: Some(profile),
            migrated: false,
        },
        Err(e) => ParseProfileResult::fail(vec![err("$", format!("画像结构不完整：{}", e))]),
    }
}

/// v2 画像迁移：只接受带真实观测的数据。
fn migrate_v2(value: &Value, now_ms: i64) -> ParseProfileResult {
    let Some(v) = value.as_object() else {
        return ParseProfileResult::fail(vec![err("version", "不是可识别的 v2 画像")]);
    };
    let model_id = match v.get("model_id").and_then(Value::as_str) {
        Some(id) if !id.is_empty() => id.to_string(),
        _ => return ParseProfileResult::fail(vec![err("version", "不是可识别的 v2 画像")]),
    };
    let ratio = num(v.get("lang_ratio")).unwrap_or(0.0);
    if ratio <= 0.0 {
        return ParseProfileResult::fail(vec![err("lang_ratio", "v2 lang_ratio 非法")]);
    }

    let Some(obs) = v.get("observations").and_then(Value::as_array) else {
        return ParseProfileResult::fail(vec![err(
            "observations",
            "v2 画像缺少真实观测，无法迁移，请重新测量",
        )]);
    };
    if obs.len() != 6 {
        return ParseProfileResult::fail(vec![err(
            "observations",
            "v2 画像缺少真实观测，无法迁移，请重新测量",
        )]);
    }
    let mut observations: Vec<Observation> = Vec::new();
    for raw in obs {
        let Some(item) = raw.as_object() else {
            return ParseProfileResult::fail(vec![err(
                "observations",
                "v2 观测字段不合法",
            )]);
        };
        let lang = match item.get("lang").and_then(Value::as_str) {
            Some("zh") => crate::types::Lang::Zh,
            Some("en") => crate::types::Lang::En,
            _ => {
                return ParseProfileResult::fail(vec![err(
                    "observations",
                    "v2 观测字段不合法",
                )])
            }
        };
        let Some(n) = num(item.get("n")).filter(|n| *n > 0.0) else {
            return ParseProfileResult::fail(vec![err(
                "observations",
                "v2 观测字段不合法",
            )]);
        };
        let Some(prompt_tokens) = num(item.get("prompt_tokens")).filter(|n| *n >= 0.0) else {
            return ParseProfileResult::fail(vec![err(
                "observations",
                "v2 观测字段不合法",
            )]);
        };
        let sample_id = item
            .get("sample_id")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| format!("v2-{}-{}", lang.name_zh(), n));
        observations.push(Observation {
            sample_id,
            lang,
            n,
            prompt_tokens,
        });
    }

    let measured_at = v
        .get("calibrated_at")
        .and_then(Value::as_str)
        .map(str::to_string)
        .unwrap_or_else(|| crate::time::iso_from_ms(now_ms));
    let measurement = match compute_calibration(
        &observations,
        &CalibrationMeta {
            method: ProfileSource::Manual,
            tokenizer: TokenizerMeta {
                name: "v2-migrated".to_string(),
                version: "2".to_string(),
                sha256: "n/a".to_string(),
                source: None,
                exact: None,
            },
            measured_at: measured_at.clone(),
            generation_ratio: None,
        },
    ) {
        Ok(mut cal) => {
            let mut warnings = vec![Warning::ProbeDegraded];
            warnings.extend(cal.warnings.drain(..));
            cal.warnings = warnings;
            cal
        }
        Err(e) => {
            return ParseProfileResult::fail(vec![err(
                "observations",
                format!("v2 观测无法用于校准：{}", e.message),
            )])
        }
    };

    let known = get_model(&model_id);
    let family = match v.get("family").and_then(Value::as_str) {
        Some("qwen") => crate::types::Family::Qwen,
        Some("deepseek") => crate::types::Family::Deepseek,
        Some("other") => crate::types::Family::Other,
        _ => family_of(&model_id),
    };
    let native_lang = match v.get("native_lang").and_then(Value::as_str) {
        Some("zh") => Some(crate::types::Lang::Zh),
        Some("en") => Some(crate::types::Lang::En),
        _ => infer_native_lang(&model_id, Some(family), Some(ratio)),
    }
    .unwrap_or(crate::types::Lang::En);
    let display_name = known
        .as_ref()
        .map(|m| m.display_name.clone())
        .unwrap_or_else(|| model_id.clone());
    let context_limit = known.as_ref().map(|m| m.context_limit).unwrap_or(65536);
    let reasoning = v
        .get("reasoning_capable")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| reasoning_capable(&model_id));

    let profile = ModelProfile {
        schema: PROFILE_SCHEMA.to_string(),
        version: PROFILE_VERSION,
        model: crate::types::ModelMeta {
            model_id,
            display_name,
            family,
            native_lang: Some(native_lang),
            reasoning_capable: reasoning,
            context_limit,
        },
        measurement,
        policy: Policy {
            mode: PolicyMode::Balanced,
            switch_threshold_tokens: 50.0,
            switch_threshold_pct: 0.08,
            ratio_high: num(v.get("ratio_high")).unwrap_or(1.35),
            ratio_low: num(v.get("ratio_low")).unwrap_or(0.85),
        },
        provenance: Provenance {
            source: ProfileSource::Manual,
            reproduce: "migrated from v2".to_string(),
            migrated: Some(true),
        },
    };

    match serde_json::to_value(&profile) {
        Ok(value) => {
            let mut result = validate_profile(&value, now_ms);
            result.migrated = true;
            result
        }
        Err(e) => ParseProfileResult::fail(vec![err("$", format!("迁移失败：{}", e))]),
    }
}

/// 解析导入文件，支持 v3 与 v2 自动迁移。
pub fn parse_profile_file(text: &str, now_ms: i64) -> ParseProfileResult {
    let value: Value = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(_) => return ParseProfileResult::fail(vec![err("$", "不是合法 JSON")]),
    };
    if value.get("schema").and_then(Value::as_str) == Some(PROFILE_SCHEMA) {
        return validate_profile(&value, now_ms);
    }
    if value.get("version").and_then(Value::as_u64) == Some(2) {
        return migrate_v2(&value, now_ms);
    }
    ParseProfileResult::fail(vec![err(
        "schema",
        "无法识别的画像格式（需要 langbridge.profile v3 或 v2）",
    )])
}
