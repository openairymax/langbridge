//! guard 子命令：对模型输出做双标签解析、漂移检测、术语替换与润色。
//!
//! 漂移只报告不改写，退出码仍为 0。

use std::io::IsTerminal;

use langbridge_core::guard::{postprocess, PostprocessOptions};
use langbridge_core::Lang;

use crate::args::ParsedArgs;
use crate::format::{line, yellow};
use crate::input::read_input;
use crate::store::CliError;

pub fn guard_command(args: &ParsedArgs) -> Result<i32, CliError> {
    let raw = read_input(args)?;
    if raw.trim().is_empty() {
        return Err(CliError::new(1, "输入为空"));
    }
    let as_json = args.flag_true("json") || !std::io::stdout().is_terminal();

    let expected_lang = match args.str("expect") {
        Some("zh") => Lang::Zh,
        Some("en") => Lang::En,
        _ => {
            return Err(CliError::new(
                1,
                "--expect zh|en 必填：期望输出的语言（来自 inject/decide 的目标语言）",
            ))
        }
    };
    let streaming = args.has("streaming");
    let result = postprocess(
        &raw,
        &PostprocessOptions {
            expected_lang,
            streaming,
        },
    );

    if as_json {
        let payload = serde_json::json!({
            "expected_lang": match expected_lang { Lang::Zh => "zh", Lang::En => "en" },
            "streaming": streaming,
            "answer": result.answer,
            "thinking": result.thinking,
            "had_tags": result.had_tags,
            "purity": result.purity,
            "drifted": result.drifted,
            "terminology_applied": result.terminology_applied,
            "polished": result.polished,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        return Ok(0);
    }

    println!("{}", result.answer);
    eprintln!(
        "{}",
        line(
            "标签",
            if result.had_tags {
                "检测到 <thinking>/<answer> 并已剥离"
            } else {
                "无标签，全文视为回答"
            }
        )
    );
    eprintln!(
        "{}",
        line(
            "纯度",
            &format!(
                "{:.1}%{}",
                result.purity * 100.0,
                if result.drifted {
                    "（低于阈值，判定漂移，不阻断）"
                } else {
                    ""
                }
            )
        )
    );
    eprintln!(
        "{}",
        line(
            "术语",
            &if streaming {
                "流式只统计不改写".to_string()
            } else {
                format!("替换 {} 处", result.terminology_applied)
            }
        )
    );
    eprintln!(
        "{}",
        line(
            "润色",
            if streaming {
                "未启用（流式）"
            } else if result.polished {
                "已执行"
            } else {
                "无需执行"
            }
        )
    );
    if result.drifted {
        eprintln!(
            "{}",
            yellow("告警：输出语言与期望不符（漂移），建议检查路由或模型行为")
        );
    }
    Ok(0)
}
