//! Tokenizer 特征校准：对同一语料的 N / 2N / 4N 三档计数做最小二乘拟合，固定开销由差分抵消。

use crate::signals::round2;
use crate::types::{
    CalibrationResult, EngineError, ErrorCode, Lang, Observation, Pair, ProfileSource,
    TokenizerMeta, Warning,
};

/// 线性度上限：二档增量相对一档增量的允许偏差，严格小于才判定为线性。
pub const LINEAR_MAX_DEVIATION: f64 = 0.15;

/// 校准置信度下限，低于该值不用于成本决策。
pub const MIN_CONFIDENCE: f64 = 0.6;

/// 每处理 100 次请求触发一次重新校准。
pub const RECALIBRATE_EVERY: u64 = 100;

#[derive(Debug, Clone, Copy)]
pub struct LinearFit {
    pub a: f64,
    pub c: f64,
    pub r2: f64,
}

fn invalid(msg: &str) -> EngineError {
    EngineError::new(ErrorCode::InvalidProfile, msg)
}

/// 最小二乘拟合 T = a + c·N
pub fn fit_linear(xs: &[f64], ys: &[f64]) -> Result<LinearFit, EngineError> {
    if xs.len() != ys.len() || xs.is_empty() {
        return Err(invalid("拟合输入长度不一致或为空"));
    }
    let n = xs.len() as f64;
    let mx = xs.iter().sum::<f64>() / n;
    let my = ys.iter().sum::<f64>() / n;
    let mut sxx = 0.0;
    let mut sxy = 0.0;
    for i in 0..xs.len() {
        sxx += (xs[i] - mx).powi(2);
        sxy += (xs[i] - mx) * (ys[i] - my);
    }
    let c = if sxx == 0.0 { 0.0 } else { sxy / sxx };
    let a = my - c * mx;
    let mut ss_tot = 0.0;
    let mut ss_res = 0.0;
    for i in 0..xs.len() {
        let pred = a + c * xs[i];
        ss_tot += (ys[i] - my).powi(2);
        ss_res += (ys[i] - pred).powi(2);
    }
    let r2 = if ss_tot == 0.0 {
        1.0
    } else {
        (1.0 - ss_res / ss_tot).clamp(0.0, 1.0)
    };
    Ok(LinearFit { a, c, r2 })
}

#[derive(Debug, Clone)]
pub struct CalibrationMeta {
    pub method: ProfileSource,
    pub tokenizer: TokenizerMeta,
    pub measured_at: String,
    pub generation_ratio: Option<f64>,
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

fn by_lang(obs: &[Observation], lang: Lang) -> Vec<Observation> {
    let mut list: Vec<Observation> = obs.iter().filter(|o| o.lang == lang).cloned().collect();
    list.sort_by(|a, b| a.n.partial_cmp(&b.n).unwrap_or(std::cmp::Ordering::Equal));
    list
}

/// 由 6 条观测（每语言 3 档）计算校准结果；观测不足时报错。
pub fn compute_calibration(
    obs: &[Observation],
    meta: &CalibrationMeta,
) -> Result<CalibrationResult, EngineError> {
    let zh = by_lang(obs, Lang::Zh);
    let en = by_lang(obs, Lang::En);
    if zh.len() < 3 || en.len() < 3 {
        return Err(invalid("校准观测必须为每语言 3 档（共 6 条）"));
    }
    let zh3 = &zh[..3];
    let en3 = &en[..3];
    let fit_zh = fit_linear(
        &zh3.iter().map(|o| o.n).collect::<Vec<_>>(),
        &zh3.iter().map(|o| o.prompt_tokens).collect::<Vec<_>>(),
    )?;
    let fit_en = fit_linear(
        &en3.iter().map(|o| o.n).collect::<Vec<_>>(),
        &en3.iter().map(|o| o.prompt_tokens).collect::<Vec<_>>(),
    )?;

    if fit_en.c <= 0.0 || fit_zh.c <= 0.0 {
        return Err(EngineError::new(
            ErrorCode::SampleTooSmall,
            "斜率非正：样本或计数不可用",
        ));
    }

    let delta1_zh = zh[1].prompt_tokens - zh[0].prompt_tokens;
    let delta2_zh = zh[2].prompt_tokens - zh[1].prompt_tokens;
    let delta1_en = en[1].prompt_tokens - en[0].prompt_tokens;
    let delta2_en = en[2].prompt_tokens - en[1].prompt_tokens;
    let dev_zh = if delta1_zh > 0.0 {
        (delta2_zh - 2.0 * delta1_zh).abs() / delta1_zh
    } else {
        f64::INFINITY
    };
    let dev_en = if delta1_en > 0.0 {
        (delta2_en - 2.0 * delta1_en).abs() / delta1_en
    } else {
        f64::INFINITY
    };
    let linearity_ok = dev_zh < LINEAR_MAX_DEVIATION && dev_en < LINEAR_MAX_DEVIATION;

    let density_ratio = fit_zh.c / fit_en.c;
    let effective_ratio = meta.generation_ratio.unwrap_or(density_ratio);
    let r2 = fit_zh.r2.min(fit_en.r2);
    let delta_min = delta1_zh.min(delta1_en);

    let mut warnings = Vec::new();
    if delta_min < 60.0 {
        warnings.push(Warning::SampleTooSmall);
    }
    if r2 < 0.95 {
        warnings.push(Warning::LowFit);
    }
    if !(0.5..=2.5).contains(&density_ratio) {
        warnings.push(Warning::Outlier);
    }

    let base = 0.45 + (delta_min / 800.0).min(0.25) + if obs.len() >= 6 { 0.1 } else { 0.0 };
    let penalty = if linearity_ok { 0.0 } else { 0.15 }
        + if meta.method == ProfileSource::Builtin {
            0.0
        } else {
            0.05
        };
    let mut confidence = clamp(base + r2 * 0.15 - penalty, 0.3, 0.95);
    if meta.generation_ratio.is_none() {
        confidence = confidence.min(0.8);
    }

    Ok(CalibrationResult {
        density_ratio: round2(density_ratio),
        generation_ratio: meta.generation_ratio.map(round2),
        effective_ratio: round2(effective_ratio),
        slopes: Pair {
            zh: round2(fit_zh.c),
            en: round2(fit_en.c),
        },
        fixed_overhead: Pair {
            zh: round2(fit_zh.a),
            en: round2(fit_en.a),
        },
        r2: round2(r2),
        linearity_ok,
        confidence: round2(confidence),
        observations: obs.to_vec(),
        method: meta.method,
        tokenizer: meta.tokenizer.clone(),
        measured_at: meta.measured_at.clone(),
        warnings,
    })
}

/// 重跑一致性：两次密度比的绝对偏差超过 15% 判定为不稳定。
pub fn compare_rerun(prev: &CalibrationResult, next: &CalibrationResult) -> (f64, bool) {
    if prev.density_ratio <= 0.0 {
        return (1.0, false);
    }
    let deviation = (next.density_ratio - prev.density_ratio).abs() / prev.density_ratio;
    (round2(deviation), deviation <= 0.15)
}

pub fn should_recalibrate(tick_count: u64) -> bool {
    tick_count > 0 && tick_count % RECALIBRATE_EVERY == 0
}

/// 判断校准结果能否用于成本决策。
pub fn calibration_usable(cal: &CalibrationResult) -> bool {
    cal.confidence >= MIN_CONFIDENCE
        && cal.linearity_ok
        && !cal.warnings.contains(&Warning::SampleTooSmall)
        && !cal.warnings.contains(&Warning::Outlier)
        && (0.5..=2.5).contains(&cal.effective_ratio)
}
