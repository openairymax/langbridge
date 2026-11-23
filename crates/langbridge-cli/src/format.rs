//! 终端输出格式化，仅在 stdout 为终端且未设置 NO_COLOR 时着色。

use std::env;
use std::io::IsTerminal;

use langbridge_core::*;

pub fn color_enabled() -> bool {
    std::io::stdout().is_terminal() && env::var_os("NO_COLOR").is_none()
}

fn paint(code: &str, text: &str) -> String {
    if color_enabled() {
        format!("\u{1b}[{}m{}\u{1b}[0m", code, text)
    } else {
        text.to_string()
    }
}

pub fn bold(t: &str) -> String {
    paint("1", t)
}
pub fn dim(t: &str) -> String {
    paint("2", t)
}
pub fn green(t: &str) -> String {
    paint("32", t)
}
pub fn yellow(t: &str) -> String {
    paint("33", t)
}
pub fn red(t: &str) -> String {
    paint("31", t)
}

fn pad_end(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        s.to_string()
    } else {
        format!("{}{}", s, " ".repeat(width - len))
    }
}

pub fn line(label: &str, value: &str) -> String {
    format!("{} {}", bold(&pad_end(label, 14)), value)
}

pub fn lang_name(l: Lang) -> &'static str {
    match l {
        Lang::Zh => "中文",
        Lang::En => "英文",
    }
}

pub fn lang_code(l: Lang) -> &'static str {
    match l {
        Lang::Zh => "zh",
        Lang::En => "en",
    }
}

pub fn task_code(t: TaskType) -> &'static str {
    match t {
        TaskType::Code => "code",
        TaskType::Math => "math",
        TaskType::Culture => "culture",
        TaskType::Policy => "policy",
        TaskType::General => "general",
    }
}

pub fn layer_label(l: RoutingLayer) -> &'static str {
    match l {
        RoutingLayer::L1 => "L1",
        RoutingLayer::L2 => "L2",
        RoutingLayer::L3 => "L3",
        RoutingLayer::L4 => "L4",
    }
}

pub fn source_code(s: ProfileSource) -> &'static str {
    match s {
        ProfileSource::Builtin => "builtin",
        ProfileSource::ApiProbe => "api_probe",
        ProfileSource::LiveRun => "live_run",
        ProfileSource::Manual => "manual",
    }
}

pub fn warning_code(w: Warning) -> &'static str {
    match w {
        Warning::LowFit => "low_fit",
        Warning::SampleTooSmall => "sample_too_small",
        Warning::Outlier => "outlier",
        Warning::ProbeDegraded => "probe_degraded",
    }
}

pub fn injection_state_code(s: InjectionState) -> &'static str {
    match s {
        InjectionState::Injected => "injected",
        InjectionState::Appended => "appended",
        InjectionState::SkippedConflict => "skipped_conflict",
        InjectionState::Disabled => "disabled",
    }
}

pub fn cost_status_code(s: CostStatus) -> &'static str {
    match s {
        CostStatus::Ok => "ok",
        CostStatus::BlockedUncalibrated => "blocked_uncalibrated",
        CostStatus::NotApplicable => "not_applicable",
        CostStatus::BlockedQuality => "blocked_quality",
        CostStatus::Negative => "negative",
    }
}

pub fn decided_by_code(d: DecidedBy) -> String {
    serde_json::to_value(d)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| "fallback".to_string())
}

pub fn js_num(n: f64) -> String {
    if !n.is_finite() {
        return "NaN".to_string();
    }
    if n.fract() == 0.0 && n.abs() < 9.007_199_254_740_992e15 {
        format!("{}", n as i64)
    } else {
        format!("{}", n)
    }
}

pub fn print_calibration(cal: &CalibrationResult) {
    println!("{}", line("方法", source_code(cal.method)));
    println!(
        "{}",
        line("density_ratio", &format!("{:.4}", cal.density_ratio))
    );
    if let Some(g) = cal.generation_ratio {
        println!("{}", line("generation_ratio", &format!("{:.4}", g)));
    }
    println!(
        "{}",
        line("effective_ratio", &format!("{:.4}", cal.effective_ratio))
    );
    println!(
        "{}",
        line(
            "斜率",
            &format!("zh={:.4} en={:.4}", cal.slopes.zh, cal.slopes.en)
        )
    );
    println!(
        "{}",
        line(
            "固定开销",
            &format!(
                "zh={:.2} en={:.2}（差分抵消）",
                cal.fixed_overhead.zh, cal.fixed_overhead.en
            )
        )
    );
    println!("{}", line("R²", &format!("{:.3}", cal.r2)));
    println!(
        "{}",
        line(
            "线性度",
            &if cal.linearity_ok {
                green("通过")
            } else {
                yellow("偏差 >15%")
            }
        )
    );
    println!(
        "{}",
        line(
            "置信度",
            &format!(
                "{:.2} {}",
                cal.confidence,
                if cal.confidence >= 0.6 {
                    green("可用")
                } else {
                    red("不足")
                }
            )
        )
    );
    println!(
        "{}",
        line(
            "tokenizer",
            &format!("{}/{}", cal.tokenizer.name, cal.tokenizer.version)
        )
    );
    if !cal.warnings.is_empty() {
        let joined = cal
            .warnings
            .iter()
            .map(|w| warning_code(*w))
            .collect::<Vec<_>>()
            .join(", ");
        println!("{}", line("告警", &yellow(&joined)));
    }
}

pub fn print_decision(
    model_id: &str,
    signals: &Signals,
    routing: &Routing,
    savings: Option<&SavingsResult>,
    injection: Option<&InjectionPlan>,
) {
    println!("{}", line("模型", model_id));
    println!(
        "{}",
        line(
            "输入",
            &format!(
                "{} {:.2} · 任务 {} · 输入≈{} tokens",
                lang_name(signals.lang),
                signals.lang_confidence,
                task_code(signals.task),
                signals.input_tokens_est
            )
        )
    );
    println!(
        "{}",
        line(
            "决策",
            &format!(
                "{}（{}）",
                bold(lang_name(routing.decided_lang)),
                decided_by_code(routing.decided_by)
            )
        )
    );
    let cs = cost_status_code(routing.cost_status);
    println!(
        "{}",
        line(
            "成本状态",
            &if routing.cost_status == CostStatus::Ok {
                green(cs)
            } else {
                yellow(cs)
            }
        )
    );
    if let Some(s) = savings {
        let net = if s.net >= 0.0 {
            green(&format!("+{}", js_num(s.net)))
        } else {
            red(&js_num(s.net))
        };
        println!(
            "{}",
            line(
                "净收益",
                &format!(
                    "{} tokens（毛收益 {} − 开销 {}；阈值 {}；{}）",
                    net,
                    js_num(s.gross),
                    js_num(s.overhead),
                    js_num(s.threshold),
                    if s.switchable {
                        "可切换"
                    } else {
                        "不建议切换"
                    }
                )
            )
        );
    } else {
        println!("{}", line("净收益", &dim("不输出成本结论")));
    }
    if let Some(inj) = injection {
        println!(
            "{}",
            line(
                "注入",
                &format!(
                    "{} · {} tokens · {}",
                    injection_state_code(inj.state),
                    js_num(inj.prompt_tokens + inj.tag_tokens),
                    inj.reason
                )
            )
        );
    }
    println!();
    println!("{}", bold("决策链"));
    for s in &routing.steps {
        let mark = if s.hit { green("●") } else { dim("○") };
        let tail = match s.decided {
            Some(l) => format!(" → {}", lang_name(l)),
            None => String::new(),
        };
        println!(
            "  {} {}{} {}",
            mark,
            layer_label(s.layer),
            tail,
            dim(&s.reason)
        );
    }
}

pub fn print_profile(profile: &ModelProfile) {
    println!(
        "{}",
        line(
            "模型",
            &format!(
                "{}（{}）",
                profile.model.model_id, profile.model.display_name
            )
        )
    );
    println!(
        "{}",
        line(
            "原生语言",
            profile.model.native_lang.map(lang_code).unwrap_or("未知")
        )
    );
    println!(
        "{}",
        line(
            "推理模型",
            if profile.model.reasoning_capable {
                "是"
            } else {
                "否"
            }
        )
    );
    print_calibration(&profile.measurement);
    println!("{}", line("来源", source_code(profile.provenance.source)));
    println!("{}", line("测量时间", &profile.measurement.measured_at));
}
