//! calibrate 子命令：用内置证据或 API 探测生成并保存画像。
//!
//! 内置证据固定 N=120。退出码 1 参数错误，2 数据不可用或质量不达标，
//! 3 上游或授权失败，4 画像校验失败。

use std::io::IsTerminal;
use std::path::Path;

use langbridge_core::calibrate::{calibration_usable, compare_rerun};
use langbridge_core::evidence::calibrate_from_evidence;
use langbridge_core::profile_store::{profile_from_calibration, serialize_profile};
use langbridge_core::CalibrationResult;

use crate::args::ParsedArgs;
use crate::format::{green, print_calibration, warning_code, yellow};
use crate::probe::{calibrate_via_api, ApiProbeOptions};
use crate::store::{read_profile, write_profile, CliError};
use crate::util::exit_code_for_error;

fn method_check(args: &ParsedArgs) -> String {
    args.str("method")
        .or_else(|| args.str("via"))
        .unwrap_or("builtin")
        .to_string()
}

pub fn calibrate_command(args: &ParsedArgs) -> Result<i32, CliError> {
    let model_id = args
        .str("model")
        .map(|s| s.to_string())
        .or_else(|| std::env::var("LANGBRIDGE_DEFAULT_MODEL").ok())
        .unwrap_or_else(|| "deepseek-reasoner".to_string());
    let method = method_check(args);
    let as_json = args.flag_true("json") || !std::io::stdout().is_terminal();
    let dry_run = args.has("dry-run");
    let out = args.str("out").map(|s| s.to_string());
    let timeout_ms = args.num("timeout").map(|n| n.max(0.0) as u64);
    let base_n = args.num("n").unwrap_or(120.0);

    if method != "builtin" && method != "api" {
        return Err(CliError::new(1, "--method 只能是 builtin 或 api"));
    }
    if base_n.fract() != 0.0 || base_n < 120.0 || (base_n as i64) % 120 != 0 {
        return Err(CliError::new(
            1,
            "--n 必须是 120 的正整数倍（内置语料以 120 为一个语义单元）",
        ));
    }
    if method == "builtin" && base_n != 120.0 {
        return Err(CliError::new(
            1,
            "内置证据固定为 N=120；自定义 --n 请使用 --method api 实测",
        ));
    }

    let mut measurement: CalibrationResult;
    if method == "api" {
        let base_url = args.str("base-url");
        let key_env = args.str("key-env").unwrap_or("LANGBRIDGE_API_KEY");
        let api_key = args
            .str("api-key")
            .map(|s| s.to_string())
            .or_else(|| std::env::var(key_env).ok());
        let Some(base_url) = base_url else {
            return Err(CliError::new(
                1,
                "API 探测需要 --base-url，例如 https://api.deepseek.com/v1",
            ));
        };
        let Some(api_key) = api_key else {
            return Err(CliError::new(
                2,
                format!("API 探测需要 --api-key 或环境变量 {}", key_env),
            ));
        };
        measurement = calibrate_via_api(&ApiProbeOptions {
            base_url,
            api_key: &api_key,
            model_id: &model_id,
            timeout_ms,
            base_n,
        })
        .map_err(|err| {
            CliError::new(
                exit_code_for_error(err.code),
                format!("校准失败（{}）：{}", crate::util::error_code_str(err.code), err.message),
            )
        })?;
    } else {
        measurement = calibrate_from_evidence(&model_id).map_err(|err| {
            CliError::new(
                exit_code_for_error(err.code),
                format!("校准失败（{}）：{}", crate::util::error_code_str(err.code), err.message),
            )
        })?;
    }

    if !calibration_usable(&measurement) {
        let detail = if measurement.warnings.is_empty() {
            "置信度不足".to_string()
        } else {
            measurement
                .warnings
                .iter()
                .map(|w| warning_code(*w))
                .collect::<Vec<_>>()
                .join(", ")
        };
        return Err(CliError::new(
            2,
            format!("校准未通过质量闸门：{}", detail),
        ));
    }

    let mut rerun: Option<(f64, f64, f64, bool)> = None;
    if args.has("rerun") {
        let prev = read_profile(&model_id).ok().flatten();
        match prev {
            None => {
                if !as_json {
                    println!("{}", yellow("--rerun：本地没有既有画像，本次结果作为首次测量"));
                }
            }
            Some(prev) => {
                let (deviation, stable) = compare_rerun(&prev.measurement, &measurement);
                rerun = Some((
                    prev.measurement.density_ratio,
                    measurement.density_ratio,
                    deviation,
                    stable,
                ));
                if !stable {
                    measurement.confidence = measurement.confidence.min(0.6);
                }
            }
        }
    }

    let profile = profile_from_calibration(&model_id, measurement, None);

    if as_json {
        if let Some((previous, current, deviation, stable)) = rerun {
            let payload = serde_json::json!({
                "rerun": {
                    "previous": previous,
                    "current": current,
                    "deviation": deviation,
                    "stable": stable,
                }
            });
            eprintln!("{}", serde_json::to_string(&payload).unwrap_or_default());
        }
        println!("{}", serialize_profile(&profile));
    } else {
        println!("{}", green("校准完成"));
        print_calibration(&profile.measurement);
        if let Some((previous, current, deviation, stable)) = rerun {
            let detail = format!(
                "偏差 {:.1}% · {}",
                deviation * 100.0,
                if stable {
                    green("stable")
                } else {
                    yellow("stable=false")
                }
            );
            println!(
                "--rerun：前次 {:.4} → 本次 {:.4} · {}",
                previous, current, detail
            );
        }
    }

    if dry_run {
        if !as_json {
            println!("{}", yellow("dry-run：未写入画像"));
        }
        return Ok(0);
    }

    let path = write_profile(&profile, out.as_deref().map(Path::new))?;
    if !as_json {
        println!("{} {}", green("已写入"), path.display());
    }
    Ok(0)
}
