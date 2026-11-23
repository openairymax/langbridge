//! 净收益：扣除注入开销后的成本差额。

use crate::profiles::{RATIO_HIGH_DEFAULT, RATIO_LOW_DEFAULT};
use crate::types::{Lang, ModelMeta, ModelProfile, SavingsResult, TaskType};

pub const SWITCH_THRESHOLD_TOKENS_DEFAULT: f64 = 50.0;
pub const SWITCH_THRESHOLD_PCT_DEFAULT: f64 = 0.08;
pub const MIN_R_SWITCH: f64 = 300.0;

fn task_base_r(task: TaskType) -> f64 {
    match task {
        TaskType::Code => 1200.0,
        TaskType::Math => 1800.0,
        TaskType::General => 1000.0,
        TaskType::Culture => 600.0,
        TaskType::Policy => 800.0,
    }
}

pub fn estimate_r(task: TaskType, model: &ModelMeta) -> f64 {
    let base = task_base_r(task);
    (base * if model.reasoning_capable { 1.5 } else { 1.0 }).round()
}

#[derive(Debug, Clone)]
pub struct SavingsInput {
    pub r: f64,
    pub current: Lang,
    pub target: Lang,
    pub ratio: f64,
    pub system_tokens: f64,
    pub tag_tokens: f64,
    pub threshold_tokens: Option<f64>,
    pub threshold_pct: Option<f64>,
}

pub fn compute_savings(input: &SavingsInput) -> SavingsResult {
    let r = input.r;
    let threshold_tokens = input
        .threshold_tokens
        .unwrap_or(SWITCH_THRESHOLD_TOKENS_DEFAULT);
    let threshold_pct = input.threshold_pct.unwrap_or(SWITCH_THRESHOLD_PCT_DEFAULT);
    let overhead = input.system_tokens.max(0.0) + input.tag_tokens.max(0.0);
    let threshold = (threshold_tokens.max(threshold_pct * r)).round();

    if input.current == input.target {
        return SavingsResult {
            r,
            current: input.current,
            target: input.target,
            ratio: input.ratio,
            multiplier: 1.0,
            gross: 0.0,
            overhead: 0.0,
            net: 0.0,
            threshold,
            switchable: false,
            reason: format!(
                "已经在用{}推理，不需要切换，也不会产生额外开销",
                input.target.name_zh()
            ),
        };
    }

    let multiplier = if input.target == Lang::Zh {
        input.ratio
    } else {
        1.0 / input.ratio
    };
    let gross = r * (1.0 - multiplier);

    if r <= MIN_R_SWITCH {
        return SavingsResult {
            r,
            current: input.current,
            target: input.target,
            ratio: input.ratio,
            multiplier,
            gross: gross.round(),
            overhead: 0.0,
            net: 0.0,
            threshold,
            switchable: false,
            reason: format!(
                "预期思考量太小（{} ≤ {} token），省下的还不够折腾，不建议切换",
                r, MIN_R_SWITCH
            ),
        };
    }

    let net = gross - overhead;
    let reason = if net <= 0.0 {
        format!(
            "切换后反而多花 {} token（毛收益 {} < 额外说明 {}）",
            (-net).round(),
            gross.round(),
            overhead.round()
        )
    } else if net < threshold {
        format!(
            "切换后净省 {} token，低于门槛 {}，不值得切换",
            net.round(),
            threshold
        )
    } else {
        format!(
            "切换后净省 {} token，超过门槛 {}，值得切换",
            net.round(),
            threshold
        )
    };

    SavingsResult {
        r,
        current: input.current,
        target: input.target,
        ratio: input.ratio,
        multiplier,
        gross: gross.round(),
        overhead: overhead.round(),
        net: net.round(),
        threshold,
        switchable: net >= threshold,
        reason,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Thresholds {
    pub threshold_tokens: f64,
    pub threshold_pct: f64,
    pub ratio_high: f64,
    pub ratio_low: f64,
}

pub fn policy_from_profile(profile: Option<&ModelProfile>) -> Thresholds {
    let stored = profile.map(|p| &p.policy);
    let legacy_band = stored
        .map(|p| {
            (p.ratio_high - 1.35).abs() < f64::EPSILON && (p.ratio_low - 0.85).abs() < f64::EPSILON
        })
        .unwrap_or(false);
    Thresholds {
        threshold_tokens: stored
            .map(|p| p.switch_threshold_tokens)
            .unwrap_or(SWITCH_THRESHOLD_TOKENS_DEFAULT),
        threshold_pct: stored
            .map(|p| p.switch_threshold_pct)
            .unwrap_or(SWITCH_THRESHOLD_PCT_DEFAULT),
        ratio_high: if legacy_band {
            RATIO_HIGH_DEFAULT
        } else {
            stored.map(|p| p.ratio_high).unwrap_or(RATIO_HIGH_DEFAULT)
        },
        ratio_low: if legacy_band {
            RATIO_LOW_DEFAULT
        } else {
            stored.map(|p| p.ratio_low).unwrap_or(RATIO_LOW_DEFAULT)
        },
    }
}
