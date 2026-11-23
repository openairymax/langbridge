//! decide 子命令：对单条输入做四层路由、净收益估算与注入决策。
//!
//! 决策结论一律返回 0，包括未校准、质量优先与不建议切换；只有参数或运行错误
//! 才返回非 0。

use std::io::IsTerminal;
use std::path::Path;

use langbridge_core::canonical::{build_canonical_request, build_decision_record, CanonicalInput};
use langbridge_core::profiles::{get_model, meta_for};
use langbridge_core::router::{route, RouteInput};
use langbridge_core::savings::estimate_r;
use langbridge_core::signals::{estimate_tokens, extract_signals};
use langbridge_core::{now_ms, ProfileRef, TaskType};

use crate::args::ParsedArgs;
use crate::format::{line, print_decision, yellow};
use crate::input::read_input;
use crate::store::{read_profile, read_profile_file, CliError};
use crate::util::{iso_from_ms, new_request_id};

const TASKS_MSG: &str = "auto 或 code|math|culture|policy|general";

fn parse_task(raw: &str) -> Option<TaskType> {
    match raw {
        "code" => Some(TaskType::Code),
        "math" => Some(TaskType::Math),
        "culture" => Some(TaskType::Culture),
        "policy" => Some(TaskType::Policy),
        "general" => Some(TaskType::General),
        _ => None,
    }
}

pub fn decide_command(args: &ParsedArgs) -> Result<i32, CliError> {
    let raw = read_input(args)?.trim().to_string();
    if raw.is_empty() {
        return Err(CliError::new(1, "输入为空"));
    }
    let as_json = args.flag_true("json") || !std::io::stdout().is_terminal();

    let explicit = match args.str("profile") {
        Some(p) if !p.is_empty() => Some(read_profile_file(Path::new(p))?),
        _ => None,
    };
    let model_id = args
        .str("model")
        .map(|s| s.to_string())
        .or_else(|| explicit.as_ref().map(|p| p.model.model_id.clone()))
        .unwrap_or_else(|| "deepseek-reasoner".to_string());
    let profile = match explicit {
        Some(p) => Some(p),
        None => read_profile(&model_id)?,
    };
    if profile.is_none() && get_model(&model_id).is_none() {
        eprintln!(
            "{}",
            yellow(&format!(
                "提示：未知模型 {}，按启发式推断原生语言与推理能力；建议先 calibrate 生成画像",
                model_id
            ))
        );
    }
    let model = meta_for(&model_id, profile.as_ref().map(|p| p.measurement.effective_ratio));

    let task_flag = args.str("task").map(|s| s.to_string());
    if let Some(t) = &task_flag {
        if t != "auto" && parse_task(t).is_none() {
            return Err(CliError::new(
                1,
                format!("--task 只能是 {}", TASKS_MSG),
            ));
        }
    }

    let mut signals = extract_signals(&raw, "");
    if let Some(t) = &task_flag {
        if t != "auto" {
            signals.task = parse_task(t).unwrap();
        }
    }

    let r_name = if args.has("R") {
        Some("R")
    } else if args.has("r") {
        Some("r")
    } else {
        None
    };
    let mut r_flag: Option<f64> = None;
    if let Some(name) = r_name {
        let raw_r = args.str(name);
        let Some(raw_r) = raw_r else {
            return Err(CliError::new(
                1,
                format!("缺少 --{} 的值：应为 1–100000 的整数，例如 --r 800", name),
            ));
        };
        if raw_r.trim().is_empty() {
            return Err(CliError::new(
                1,
                format!("缺少 --{} 的值：应为 1–100000 的整数，例如 --r 800", name),
            ));
        }
        let n: f64 = raw_r.trim().parse().unwrap_or(f64::NAN);
        if !n.is_finite() || n < 1.0 || n > 100000.0 {
            return Err(CliError::new(
                1,
                format!(
                    "--{} 取值不合法：{}（应为 1–100000 的整数，表示预期推理规模）",
                    name, raw_r
                ),
            ));
        }
        r_flag = Some(n);
    }
    let r = r_flag.unwrap_or_else(|| estimate_r(signals.task, &model));
    let result = route(&RouteInput {
        signals: &signals,
        model: &model,
        profile: profile.as_ref(),
        r,
        streaming: args.has("streaming"),
        user_system_prompt: args.str("system-prompt"),
        now_ms: now_ms(),
    });

    let now = now_ms();
    let record = build_decision_record(
        &model_id,
        signals.clone(),
        result.routing.clone(),
        result.savings.clone(),
        Some(result.injection.clone()),
        ProfileRef {
            version: profile.as_ref().map(|p| p.version),
            source: profile.as_ref().map(|p| p.provenance.source),
            measured_at: profile
                .as_ref()
                .map(|p| p.measurement.measured_at.clone()),
        },
        iso_from_ms(now),
    );
    let canonical = build_canonical_request(CanonicalInput {
        raw_input: &raw,
        signals: signals.clone(),
        routing: result.routing.clone(),
        injection: Some(result.injection.clone()),
        savings: result.savings.clone(),
        final_output: None,
        request_id: new_request_id(),
        created_at: iso_from_ms(now),
    });

    if as_json {
        let payload = serde_json::json!({
            "record": record,
            "canonical": canonical,
            "input_tokens_est": estimate_tokens(&raw),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        print_decision(
            &model_id,
            &signals,
            &result.routing,
            result.savings.as_ref(),
            Some(&result.injection),
        );
        if profile.is_none() {
            println!(
                "{}",
                yellow(&format!(
                    "提示：未找到画像，先运行 langbridge calibrate --model {}",
                    model_id
                ))
            );
        }
        println!(
            "{}",
            line(
                "输入估算",
                &format!("{} tokens（启发式，仅展示）", signals.input_tokens_est)
            )
        );
    }

    Ok(0)
}
