//! inject 子命令：输出单次请求的语言约束注入计划，供脚本编排。

use std::io::IsTerminal;
use std::path::Path;

use langbridge_core::profiles::meta_for;
use langbridge_core::router::{route, RouteInput};
use langbridge_core::savings::estimate_r;
use langbridge_core::signals::{estimate_tokens, extract_signals};
use langbridge_core::{now_ms, TaskType};

use crate::args::ParsedArgs;
use crate::format::{lang_code, yellow};
use crate::input::read_input;
use crate::store::{read_profile, read_profile_file, CliError};

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

pub fn inject_command(args: &ParsedArgs) -> Result<i32, CliError> {
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
    let model = meta_for(
        &model_id,
        profile.as_ref().map(|p| p.measurement.effective_ratio),
    );

    let task_flag = args.str("task").map(|s| s.to_string());
    if let Some(t) = &task_flag {
        if t != "auto" && parse_task(t).is_none() {
            return Err(CliError::new(
                1,
                "--task 只能是 auto 或 code|math|culture|policy|general",
            ));
        }
    }

    let mut signals = extract_signals(&raw, "");
    if let Some(t) = &task_flag {
        if t != "auto" {
            signals.task = parse_task(t).unwrap();
        }
    }

    let r_flag = args.num("R").or_else(|| args.num("r"));
    if let Some(n) = r_flag {
        if !(1.0..=100000.0).contains(&n) {
            return Err(CliError::new(1, "--r 取值范围为 1–100000"));
        }
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

    if as_json {
        let payload = serde_json::json!({
            "model_id": model_id,
            "output_lang": lang_code(result.routing.output_lang),
            "injection": result.injection,
            "input_tokens_est": estimate_tokens(&raw),
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        return Ok(0);
    }

    let plan = &result.injection;
    if !plan.system_prompt.is_empty() {
        println!("{}", plan.system_prompt);
    }
    let status = if !plan.system_prompt.is_empty() {
        if args.has("streaming") {
            "流式协议注入".to_string()
        } else {
            "已注入".to_string()
        }
    } else {
        format!("未注入（{}）", plan.reason)
    };
    eprintln!(
        "注入状态：{} · 目标语言 {}",
        status,
        lang_code(plan.target_lang)
    );
    if profile.is_none() {
        eprintln!(
            "{}",
            yellow(&format!(
                "提示：未找到画像，先运行 langbridge setup 或 langbridge calibrate --model {}",
                model_id
            ))
        );
    }
    Ok(0)
}
