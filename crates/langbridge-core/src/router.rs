//! 四层路由：L1 硬规则、L2 模型能力、L3 输入对齐、L4 成本闸门。
//! 层序不可调换。未通过成本闸门时不输出成本结论。

use crate::calibrate::{calibration_usable, MIN_CONFIDENCE};
use crate::inject::{build_injection_plan, build_system_prompt, has_language_conflict, InjectionInput, PromptOptions};
use crate::profile_store::{is_expired, PROFILE_MAX_AGE_DAYS};
use crate::savings::{compute_savings, policy_from_profile, SavingsInput};
use crate::types::{
    CostStatus, DecidedBy, EngineError, EngineResult, ErrorCode, InjectionPlan, InjectionState, Lang,
    ModelMeta, ModelProfile, Routing, RoutingLayer, RoutingStep, SavingsResult, Signals,
};

#[derive(Debug, Clone)]
pub struct RouteInput<'a> {
    pub signals: &'a Signals,
    pub model: &'a ModelMeta,
    pub profile: Option<&'a ModelProfile>,
    pub r: f64,
    pub streaming: bool,
    pub user_system_prompt: Option<&'a str>,
    /// 当前时间（Unix 毫秒），由调用方传入，核心不直接读取平台时钟
    pub now_ms: i64,
}

#[derive(Debug, Clone)]
pub struct RouteResult {
    pub routing: Routing,
    pub savings: Option<SavingsResult>,
    pub injection: InjectionPlan,
}

pub fn is_profile_expired(profile: &ModelProfile, now_ms: i64) -> bool {
    is_expired(profile, now_ms)
}

fn step(
    layer: RoutingLayer,
    hit: bool,
    reason: impl Into<String>,
    confidence: f64,
    terminated: bool,
    decided: Option<Lang>,
) -> RoutingStep {
    RoutingStep {
        layer,
        hit,
        decided,
        reason: reason.into(),
        confidence,
        terminated,
    }
}

fn terminated_steps(from: RoutingLayer, by: &str) -> Vec<RoutingStep> {
    const ALL: [RoutingLayer; 4] = [RoutingLayer::L1, RoutingLayer::L2, RoutingLayer::L3, RoutingLayer::L4];
    let start = ALL.iter().position(|l| *l == from).unwrap_or(0);
    ALL[start..]
        .iter()
        .map(|l| step(*l, false, format!("由 {} 终止，未评估", by), 0.0, true, None))
        .collect()
}

pub fn route(input: &RouteInput<'_>) -> RouteResult {
    let signals = input.signals;
    let model = input.model;
    let mut steps: Vec<RoutingStep> = Vec::new();
    let output_lang = signals.lang;
    // 用户 system prompt 已规定语言时不注入，冲突下不保留收益结论
    let conflict = has_language_conflict(input.user_system_prompt);

    let finish = |steps: Vec<RoutingStep>,
                  decided: Lang,
                  decided_by: DecidedBy,
                  quality_first: bool,
                  cost_status: CostStatus,
                  computed: Option<SavingsResult>|
     -> RouteResult {
        let routing = Routing {
            decided_lang: decided,
            decided_by,
            output_lang,
            quality_first,
            cost_status,
            steps,
        };
        let injection = build_injection_plan(&InjectionInput {
            model,
            profile: input.profile,
            signals,
            routing: &routing,
            savings: computed.as_ref(),
            user_system_prompt: input.user_system_prompt,
            streaming: input.streaming,
        });
        let injects = matches!(
            injection.state,
            InjectionState::Injected | InjectionState::Appended
        );
        // 只有实际注入时才报告收益；冲突时不能把注入开销记为 0
        let savings = if injection.state == InjectionState::SkippedConflict
            || (!injects && computed.as_ref().map(|s| s.switchable).unwrap_or(false))
        {
            None
        } else {
            computed
        };
        RouteResult {
            routing,
            savings,
            injection,
        }
    };

    // L1 硬规则：文化、政策类任务强制中文推理
    if matches!(signals.task, crate::types::TaskType::Culture | crate::types::TaskType::Policy) {
        let label = if signals.task == crate::types::TaskType::Culture {
            "文化类（成语/典故/诗词等）"
        } else {
            "政策类（政策/法规/公文等）"
        };
        steps.push(step(
            RoutingLayer::L1,
            true,
            format!("任务为{}，质量/合规优先，强制中文推理", label),
            0.99,
            true,
            Some(Lang::Zh),
        ));
        steps.extend(terminated_steps(RoutingLayer::L2, "L1"));
        return finish(steps, Lang::Zh, DecidedBy::L1, true, CostStatus::BlockedQuality, None);
    }
    steps.push(step(
        RoutingLayer::L1,
        false,
        "任务非文化/政策类，不触发硬规则",
        0.9,
        false,
        None,
    ));

    // L2 模型能力：Qwen / DeepSeek 系列处理代码或数学任务时使用中文推理
    if matches!(model.family, crate::types::Family::Qwen | crate::types::Family::Deepseek)
        && matches!(signals.task, crate::types::TaskType::Code | crate::types::TaskType::Math)
    {
        steps.push(step(
            RoutingLayer::L2,
            true,
            format!(
                "{} 属 {:?} 系列，中文在代码/数学任务上推理更优",
                model.display_name, model.family
            ),
            0.85,
            true,
            Some(Lang::Zh),
        ));
        steps.extend(terminated_steps(RoutingLayer::L3, "L2"));
        return finish(steps, Lang::Zh, DecidedBy::L2, true, CostStatus::BlockedQuality, None);
    }
    steps.push(step(
        RoutingLayer::L2,
        false,
        if model.family == crate::types::Family::Other {
            "非国产系列模型，不触发能力规则"
        } else {
            "任务非代码/数学类，不触发能力规则"
        },
        0.7,
        false,
        None,
    ));

    // L3 输入对齐：输入英文且比值高于上限时用英文，输入中文且低于下限时用中文。
    let native = input.profile.and_then(|p| p.model.native_lang).or(model.native_lang);
    let policy = policy_from_profile(input.profile);
    let usable = input
        .profile
        .map(|p| {
            p.version == 3
                && p.model.model_id == model.model_id
                && !is_profile_expired(p, input.now_ms)
                && calibration_usable(&p.measurement)
        })
        .unwrap_or(false);

    let mut align_to: Option<Lang> = None;
    let mut align_reason = String::new();
    let active = if usable { input.profile } else { None };
    if let Some(active) = active {
        let ratio = active.measurement.effective_ratio;
        if signals.lang == Lang::En && ratio > policy.ratio_high {
            align_to = Some(Lang::En);
            align_reason = format!(
                "输入为英文且中英文成本比 {} > {}，跟随输入推理可免翻译开销",
                ratio, policy.ratio_high
            );
        } else if signals.lang == Lang::Zh && ratio < policy.ratio_low {
            align_to = Some(Lang::Zh);
            align_reason = format!(
                "输入为中文且中英文成本比 {} < {}，跟随输入推理可免翻译开销",
                ratio, policy.ratio_low
            );
        }
    } else if let Some(native) = native {
        if signals.lang != native && signals.lang_confidence >= 0.6 {
            align_to = Some(signals.lang);
            align_reason = format!(
                "尚未完成可用校准，降级为跟随输入语言（{}，置信度 {:.2}）",
                signals.lang.name_zh(),
                signals.lang_confidence
            );
        }
    }

    if let Some(align_to) = align_to {
        if !model.reasoning_capable {
            steps.push(step(RoutingLayer::L3, true, align_reason, 0.9, true, Some(align_to)));
            steps.push(step(RoutingLayer::L4, false, "由 L3 终止，未评估成本", 0.0, true, None));
            return finish(steps, align_to, DecidedBy::L3, false, CostStatus::NotApplicable, None);
        }
        if active.is_none() {
            steps.push(step(RoutingLayer::L3, true, align_reason, 0.9, true, Some(align_to)));
            steps.push(step(RoutingLayer::L4, false, "由 L3 终止，未评估成本", 0.0, true, None));
            return finish(steps, align_to, DecidedBy::L3, false, CostStatus::BlockedUncalibrated, None);
        }
        // 成本基准是输入语言，跟随输入即默认行为，无需切换，也没有注入开销
        steps.push(step(RoutingLayer::L3, true, align_reason, 0.9, true, Some(align_to)));
        steps.push(step(RoutingLayer::L4, false, "由 L3 终止，未评估成本", 0.0, true, None));
        return finish(steps, align_to, DecidedBy::L3, false, CostStatus::Ok, None);
    }
    steps.push(step(
        RoutingLayer::L3,
        false,
        if native.is_some() {
            format!(
                "输入语言未触发对齐阈值（比值在 [{}, {}] 内或语言一致），进入成本决策",
                policy.ratio_low, policy.ratio_high
            )
        } else {
            "模型原生语言未知，跳过输入对齐".to_string()
        },
        0.6,
        false,
        None,
    ));

    // L4 成本闸门
    if !model.reasoning_capable {
        steps.push(step(
            RoutingLayer::L4,
            false,
            "非推理模型：没有独立推理 token，本工具不适用",
            0.9,
            true,
            None,
        ));
        return finish(steps, signals.lang, DecidedBy::L4Blocked, false, CostStatus::NotApplicable, None);
    }

    let Some(profile) = input.profile else {
        steps.push(step(
            RoutingLayer::L4,
            false,
            "模型未校准：成本结论已停用，仅按质量规则选择推理语言",
            0.9,
            true,
            None,
        ));
        return finish(
            steps,
            signals.lang,
            DecidedBy::L4Blocked,
            false,
            CostStatus::BlockedUncalibrated,
            None,
        );
    };
    if profile.version != 3 {
        steps.push(step(
            RoutingLayer::L4,
            false,
            "模型未校准：成本结论已停用，仅按质量规则选择推理语言",
            0.9,
            true,
            None,
        ));
        return finish(
            steps,
            signals.lang,
            DecidedBy::L4Blocked,
            false,
            CostStatus::BlockedUncalibrated,
            None,
        );
    }

    if profile.model.model_id != model.model_id {
        steps.push(step(
            RoutingLayer::L4,
            false,
            format!(
                "画像属于 {}，与当前模型不一致",
                profile.model.model_id
            ),
            0.9,
            true,
            None,
        ));
        return finish(
            steps,
            signals.lang,
            DecidedBy::L4Blocked,
            false,
            CostStatus::BlockedUncalibrated,
            None,
        );
    }

    let cal = &profile.measurement;
    let now_ms = input.now_ms;
    // 置信度也是 calibration_usable 的条件；先单独拦截是为给出更具体的提示
    if cal.confidence < MIN_CONFIDENCE {
        steps.push(step(
            RoutingLayer::L4,
            false,
            format!(
                "校准置信度不足（{:.2} < {:.2}），成本结论不可用",
                cal.confidence, MIN_CONFIDENCE
            ),
            0.6,
            true,
            None,
        ));
        return finish(
            steps,
            signals.lang,
            DecidedBy::L4Blocked,
            false,
            CostStatus::BlockedUncalibrated,
            None,
        );
    }
    if !calibration_usable(cal) {
        let detail = if cal.warnings.is_empty() {
            "校准质量不达标".to_string()
        } else {
            format!("告警：{:?}", cal.warnings)
        };
        steps.push(step(
            RoutingLayer::L4,
            false,
            format!("{}，成本结论不可用", detail),
            0.6,
            true,
            None,
        ));
        return finish(
            steps,
            signals.lang,
            DecidedBy::L4Blocked,
            false,
            CostStatus::BlockedUncalibrated,
            None,
        );
    }
    if is_profile_expired(profile, now_ms) {
        steps.push(step(
            RoutingLayer::L4,
            false,
            format!("画像已超过 {} 天，需重新校准后才能给出成本结论", PROFILE_MAX_AGE_DAYS),
            0.6,
            true,
            None,
        ));
        return finish(
            steps,
            signals.lang,
            DecidedBy::L4Blocked,
            false,
            CostStatus::BlockedUncalibrated,
            None,
        );
    }

    let ratio = cal.effective_ratio;
    let (target, rule) = if ratio > policy.ratio_high {
        (
            Lang::En,
            format!(
                "中英文成本比 {} > 上限 {}，英文推理更省 Token",
                ratio, policy.ratio_high
            ),
        )
    } else if ratio < policy.ratio_low {
        (
            Lang::Zh,
            format!(
                "中英文成本比 {} < 下限 {}，中文推理更省 Token",
                ratio, policy.ratio_low
            ),
        )
    } else {
        let fallback = native.unwrap_or(if ratio <= 1.0 { Lang::Zh } else { Lang::En });
        let why = match native {
            Some(n) => format!(
                "中英文成本比 {} 处于 [{}, {}] 中庸区间，按原生语言（{}）与输入语言的成本差裁决",
                ratio,
                policy.ratio_low,
                policy.ratio_high,
                n.name_zh()
            ),
            None => format!(
                "中英文成本比 {} 处于中庸区间且原生语言未知，按效率方向选择",
                ratio
            ),
        };
        (fallback, why)
    };
    let baseline = signals.lang;
    let built = build_system_prompt(&PromptOptions {
        reasoning_lang: target,
        output_lang,
        streaming: input.streaming,
        use_tags: None,
    });
    let savings = compute_savings(&SavingsInput {
        r: input.r,
        current: baseline,
        target,
        ratio,
        system_tokens: built.prompt_tokens,
        tag_tokens: built.tag_tokens,
        threshold_tokens: Some(policy.threshold_tokens),
        threshold_pct: Some(policy.threshold_pct),
    });

    if conflict {
        steps.push(step(
            RoutingLayer::L4,
            true,
            "用户 system prompt 已规定语言，跳过注入，不产生可量化收益",
            0.7,
            true,
            Some(baseline),
        ));
        return finish(steps, baseline, DecidedBy::L4, false, CostStatus::Ok, Some(savings));
    }

    if savings.switchable {
        steps.push(step(
            RoutingLayer::L4,
            true,
            format!(
                "{}；净收益 {} > 阈值 {}，切换为{}推理",
                rule,
                savings.net,
                savings.threshold,
                target.name_zh()
            ),
            0.8,
            true,
            Some(target),
        ));
        return finish(steps, target, DecidedBy::L4, false, CostStatus::Ok, Some(savings));
    }

    steps.push(step(
        RoutingLayer::L4,
        true,
        format!(
            "{}；{}，保持{}推理",
            rule,
            savings.reason,
            baseline.name_zh()
        ),
        0.7,
        true,
        Some(baseline),
    ));
    let no_switch = target == baseline;
    let status = if no_switch || savings.net > 0.0 {
        CostStatus::Ok
    } else {
        CostStatus::Negative
    };
    finish(steps, baseline, DecidedBy::L4, false, status, Some(savings))
}

/// CLI 与代理使用的统一入口。
pub fn route_or_throw(input: &RouteInput<'_>) -> EngineResult<RouteResult> {
    if input.signals.input_chars == 0 {
        return Err(EngineError::new(ErrorCode::EmptyInput, "输入为空，无法提取信号"));
    }
    Ok(route(input))
}
