use langbridge_core::calibrate::{calibration_usable, compute_calibration, CalibrationMeta};
use langbridge_core::evidence::{
    calibrate_from_evidence, normalize_base, parse_usage, probe_steps, request_target,
};
use langbridge_core::guard::{parse_output, postprocess, PostprocessOptions};
use langbridge_core::inject::{build_injection_plan, has_language_conflict, InjectionInput};
use langbridge_core::profiles::{build_profile, get_model, meta_for};
use langbridge_core::router::{route, RouteInput};
use langbridge_core::savings::{
    compute_savings, estimate_r, SavingsInput, MIN_R_SWITCH,
};
use langbridge_core::signals::{detect_task, extract_signals, estimate_tokens};
use langbridge_core::time::{iso_from_ms, parse_iso_ms};
use langbridge_core::types::{
    CalibrationResult, CostStatus, DecidedBy, InjectionState, Lang, ModelProfile, Observation,
    ProfileSource, TaskType, TokenizerMeta,
};

const T0: &str = "2026-11-21T13:58:35.413Z";

fn tokenizer() -> TokenizerMeta {
    TokenizerMeta {
        name: "test".into(),
        version: "1".into(),
        sha256: "n/a".into(),
        source: None,
        exact: None,
    }
}

/// 中文斜率 2、英文斜率 1，固定开销 100：密度比应恰为 2
fn synthetic_obs() -> Vec<Observation> {
    let mut obs = Vec::new();
    for n in [120.0, 240.0, 480.0] {
        obs.push(Observation {
            sample_id: format!("zh-{}", n),
            lang: Lang::Zh,
            n,
            prompt_tokens: 100.0 + n * 2.0,
        });
        obs.push(Observation {
            sample_id: format!("en-{}", n),
            lang: Lang::En,
            n,
            prompt_tokens: 100.0 + n,
        });
    }
    obs
}

fn calibrated(ratio_zh: f64, ratio_en: f64) -> CalibrationResult {
    let mut obs = Vec::new();
    for n in [120.0, 240.0, 480.0] {
        obs.push(Observation {
            sample_id: format!("zh-{}", n),
            lang: Lang::Zh,
            n,
            prompt_tokens: 100.0 + n * ratio_zh,
        });
        obs.push(Observation {
            sample_id: format!("en-{}", n),
            lang: Lang::En,
            n,
            prompt_tokens: 100.0 + n * ratio_en,
        });
    }
    compute_calibration(
        &obs,
        &CalibrationMeta {
            method: ProfileSource::ApiProbe,
            tokenizer: tokenizer(),
            measured_at: T0.into(),
            generation_ratio: None,
        },
    )
    .expect("合成数据应可校准")
}

fn profile(theme: f64) -> ModelProfile {
    let model = get_model("deepseek-reasoner").unwrap_or_else(|| meta_for("deepseek-reasoner", None));
    build_profile(model, calibrated(theme, 1.0))
}

fn now() -> i64 {
    parse_iso_ms(T0).unwrap() + 24 * 3600 * 1000
}

#[test]
fn detects_chinese_and_english() {
    let zh = extract_signals("请帮我分析这段中文材料的论证结构，并指出其中的逻辑问题。", "");
    assert_eq!(zh.lang, Lang::Zh);
    let en = extract_signals("Please analyze the argument structure of this passage and list its flaws.", "");
    assert_eq!(en.lang, Lang::En);
    let empty = extract_signals("", "");
    assert_eq!(empty.lang, Lang::En);
    assert!(empty.lang_confidence <= 0.35);
}

#[test]
fn code_blocks_do_not_change_language() {
    let text = "用中文解释：\n```rust\nfn main() { println!(\"hello world\"); }\n```";
    let s = extract_signals(text, "");
    assert_eq!(s.lang, Lang::Zh);
}

#[test]
fn detects_tasks() {
    assert_eq!(detect_task("写一个快速排序函数").task, TaskType::Code);
    assert_eq!(detect_task("证明根号二是无理数").task, TaskType::Math);
    assert_eq!(detect_task("解释一下这个成语的典故").task, TaskType::Culture);
    assert_eq!(detect_task("这条法规规定了什么合规要求").task, TaskType::Policy);
    assert_eq!(detect_task("今天天气怎么样").task, TaskType::General);
}

#[test]
fn token_estimate_has_floor() {
    assert_eq!(estimate_tokens(""), 1);
    assert!(estimate_tokens("你好世界") >= 3);
}

#[test]
fn calibration_recovers_ratio_and_confidence() {
    let cal = compute_calibration(
        &synthetic_obs(),
        &CalibrationMeta {
            method: ProfileSource::Builtin,
            tokenizer: tokenizer(),
            measured_at: T0.into(),
            generation_ratio: None,
        },
    )
    .unwrap();
    assert!((cal.density_ratio - 2.0).abs() < 0.01);
    assert!(cal.linearity_ok);
    assert!(cal.r2 > 0.99);
    assert!(calibration_usable(&cal));
    assert_eq!(cal.fixed_overhead.zh, 100.0);
}

#[test]
fn calibration_rejects_short_observations() {
    let only_zh: Vec<Observation> = synthetic_obs()
        .into_iter()
        .filter(|o| o.lang == Lang::Zh)
        .collect();
    assert!(compute_calibration(
        &only_zh,
        &CalibrationMeta {
            method: ProfileSource::ApiProbe,
            tokenizer: tokenizer(),
            measured_at: T0.into(),
            generation_ratio: None,
        },
    )
    .is_err());
}

#[test]
fn savings_respect_threshold_and_overhead() {
    let on = compute_savings(&SavingsInput {
        r: 1500.0,
        current: Lang::Zh,
        target: Lang::En,
        ratio: 2.0,
        system_tokens: 40.0,
        tag_tokens: 4.0,
        threshold_tokens: Some(50.0),
        threshold_pct: Some(0.08),
    });
    assert!(on.switchable);
    assert_eq!(on.gross, 750.0);
    assert_eq!(on.net, 706.0);

    let same = compute_savings(&SavingsInput {
        r: 1500.0,
        current: Lang::Zh,
        target: Lang::Zh,
        ratio: 2.0,
        system_tokens: 40.0,
        tag_tokens: 4.0,
        threshold_tokens: None,
        threshold_pct: None,
    });
    assert!(!same.switchable);
    assert_eq!(same.overhead, 0.0);
    assert_eq!(same.net, 0.0);

    let tiny = compute_savings(&SavingsInput {
        r: MIN_R_SWITCH,
        current: Lang::Zh,
        target: Lang::En,
        ratio: 2.0,
        system_tokens: 40.0,
        tag_tokens: 0.0,
        threshold_tokens: None,
        threshold_pct: None,
    });
    assert!(!tiny.switchable);
    assert_eq!(tiny.overhead, 0.0);
}

#[test]
fn reasoning_models_expect_more_tokens() {
    let reasoner = meta_for("deepseek-reasoner", None);
    let chat = meta_for("deepseek-chat", None);
    assert_eq!(estimate_r(TaskType::General, &reasoner), 1500.0);
    assert_eq!(estimate_r(TaskType::General, &chat), 1000.0);
}

fn route_general(text: &str, model_id: &str, p: Option<&ModelProfile>, system: Option<&str>) -> langbridge_core::router::RouteResult {
    let signals = extract_signals(text, "");
    let model = meta_for(model_id, None);
    let r = estimate_r(signals.task, &model);
    route(&RouteInput {
        signals: &signals,
        model: &model,
        profile: p,
        r,
        streaming: false,
        user_system_prompt: system,
        now_ms: now(),
    })
}

#[test]
fn culture_task_forces_chinese() {
    let res = route_general("解释一下这个成语的典故", "gpt-4o", None, None);
    assert_eq!(res.routing.decided_lang, Lang::Zh);
    assert_eq!(res.routing.decided_by, DecidedBy::L1);
    assert_eq!(res.routing.cost_status, CostStatus::BlockedQuality);
    assert!(res.savings.is_none());
}

#[test]
fn chinese_family_code_task_forces_chinese() {
    let res = route_general("写一个快速排序函数", "deepseek-reasoner", None, None);
    assert_eq!(res.routing.decided_by, DecidedBy::L2);
    assert_eq!(res.routing.decided_lang, Lang::Zh);
    assert_eq!(res.routing.cost_status, CostStatus::BlockedQuality);
}

#[test]
fn non_reasoning_model_is_not_applicable() {
    let res = route_general("今天天气怎么样", "gpt-4o", None, None);
    assert_eq!(res.routing.cost_status, CostStatus::NotApplicable);
    assert_eq!(res.injection.state, InjectionState::Disabled);
}

#[test]
fn uncalibrated_model_is_blocked() {
    let res = route_general("今天天气怎么样", "deepseek-reasoner", None, None);
    assert_eq!(res.routing.cost_status, CostStatus::BlockedUncalibrated);
    assert_eq!(res.routing.decided_by, DecidedBy::L4Blocked);
    assert!(res.savings.is_none());
}

#[test]
fn high_ratio_switches_to_english_when_worth_it() {
    let p = profile(2.0);
    let res = route_general("今天天气怎么样", "deepseek-reasoner", Some(&p), None);
    assert_eq!(res.routing.decided_lang, Lang::En);
    assert_eq!(res.routing.decided_by, DecidedBy::L4);
    assert_eq!(res.injection.state, InjectionState::Injected);
    assert!(res.savings.as_ref().unwrap().switchable);
}

#[test]
fn flat_ratio_keeps_native_language() {
    let p = profile(1.0);
    let res = route_general("今天天气怎么样", "deepseek-reasoner", Some(&p), None);
    assert_eq!(res.routing.decided_lang, Lang::Zh);
    assert_eq!(res.injection.state, InjectionState::Disabled);
}

#[test]
fn english_input_aligns_to_input() {
    let p = profile(2.0);
    let res = route_general("Tell me about the weather in your city today.", "deepseek-reasoner", Some(&p), None);
    assert_eq!(res.routing.decided_by, DecidedBy::L3);
    assert_eq!(res.routing.decided_lang, Lang::En);
}

#[test]
fn system_prompt_conflict_removes_savings() {
    let p = profile(2.0);
    let res = route_general(
        "今天天气怎么样",
        "deepseek-reasoner",
        Some(&p),
        Some("Always answer in Chinese."),
    );
    assert_eq!(res.injection.state, InjectionState::SkippedConflict);
    assert!(res.savings.is_none());
}

#[test]
fn conflict_detection_is_narrow() {
    assert!(has_language_conflict(Some("Please answer in Chinese.")));
    assert!(has_language_conflict(Some("请用中文回答")));
    assert!(!has_language_conflict(Some("You are a language model expert.")));
    assert!(!has_language_conflict(None));
}

#[test]
fn injection_disabled_when_not_worth_it() {
    let model = meta_for("deepseek-reasoner", None);
    let signals = extract_signals("今天天气怎么样", "");
    let p = profile(1.0);
    let routing = langbridge_core::types::Routing {
        decided_lang: Lang::Zh,
        decided_by: DecidedBy::L4,
        output_lang: Lang::Zh,
        quality_first: false,
        cost_status: CostStatus::Ok,
        steps: vec![],
    };
    let savings = compute_savings(&SavingsInput {
        r: 1000.0,
        current: Lang::Zh,
        target: Lang::Zh,
        ratio: 1.0,
        system_tokens: 40.0,
        tag_tokens: 0.0,
        threshold_tokens: None,
        threshold_pct: None,
    });
    let plan = build_injection_plan(&InjectionInput {
        model: &model,
        profile: Some(&p),
        signals: &signals,
        routing: &routing,
        savings: Some(&savings),
        user_system_prompt: None,
        streaming: false,
    });
    assert_eq!(plan.state, InjectionState::Disabled);
}

#[test]
fn streaming_prompt_has_no_tags() {
    let model = meta_for("deepseek-reasoner", None);
    let signals = extract_signals("今天天气怎么样", "");
    let routing = langbridge_core::types::Routing {
        decided_lang: Lang::En,
        decided_by: DecidedBy::L4,
        output_lang: Lang::Zh,
        quality_first: true,
        cost_status: CostStatus::Ok,
        steps: vec![],
    };
    let plan = build_injection_plan(&InjectionInput {
        model: &model,
        profile: None,
        signals: &signals,
        routing: &routing,
        savings: None,
        user_system_prompt: None,
        streaming: true,
    });
    assert!(!plan.system_prompt.contains("<thinking>"));
    assert_eq!(plan.tag_tokens, 0.0);
}

#[test]
fn guard_parses_tags_and_postprocesses() {
    let raw = "<thinking>internal notes</thinking><answer>这是   一个答案</answer>";
    let parsed = parse_output(raw);
    assert_eq!(parsed.thinking, "internal notes");
    assert_eq!(parsed.answer, "这是   一个答案");
    assert!(parsed.had_tags);
    let out = postprocess(
        "<answer>Please   give   the answer.</answer>",
        &PostprocessOptions {
            expected_lang: Lang::En,
            streaming: false,
        },
    );
    assert_eq!(out.answer, "Please give the answer.");
}

#[test]
fn builtin_evidence_calibrates() {
    let cal = calibrate_from_evidence("deepseek-reasoner").expect("内置证据应可用");
    assert!(calibration_usable(&cal));
    assert!(cal.density_ratio > 0.5 && cal.density_ratio < 1.2);
    assert!(calibrate_from_evidence("not-a-model").is_err());
}

#[test]
fn base_url_normalization() {
    assert_eq!(
        normalize_base("api.deepseek.com"),
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(
        normalize_base("https://api.deepseek.com/v1/"),
        "https://api.deepseek.com/v1/chat/completions"
    );
    assert_eq!(
        normalize_base("http://localhost:11434/v1"),
        "http://localhost:11434/v1/chat/completions"
    );
    assert!(normalize_base("https://api.deepseek.com/v1/chat/completions")
        .ends_with("/v1/chat/completions"));
    assert_eq!(normalize_base(""), "");
}

#[test]
fn azure_requires_api_version() {
    let err = request_target(
        "https://demo.openai.azure.com/openai/deployments/x/chat/completions",
        "k",
    )
    .unwrap_err();
    assert_eq!(err.code, langbridge_core::types::ErrorCode::BadBaseUrl);
    let ok = request_target(
        "https://demo.openai.azure.com/openai/deployments/x/chat/completions?api-version=2024-10-21",
        "k",
    )
    .unwrap();
    assert!(ok.headers.iter().any(|(k, _)| k == "api-key"));
}

#[test]
fn usage_parsing_handles_json_and_sse() {
    let json = r#"{"usage":{"prompt_tokens":123}}"#;
    assert_eq!(parse_usage(json).prompt_tokens, Some(123.0));
    let sse = "data: {\"usage\":{\"prompt_tokens\":77}}\n\ndata: [DONE]\n";
    assert_eq!(parse_usage(sse).prompt_tokens, Some(77.0));
    let err = r#"{"error":{"message":"invalid api key"}}"#;
    assert!(parse_usage(err).error_message.is_some());
}

#[test]
fn probe_has_six_steps() {
    let steps = probe_steps(120.0);
    assert_eq!(steps.len(), 6);
    assert_eq!(steps.first().unwrap().lang, Lang::Zh);
    assert_eq!(steps.last().unwrap().n, 480.0);
}

#[test]
fn iso_time_roundtrip() {
    let ms = parse_iso_ms(T0).unwrap();
    assert_eq!(iso_from_ms(ms), T0);
    assert!(parse_iso_ms("not-a-time").is_none());
}
