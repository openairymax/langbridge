//! setup 子命令：用内置证据校准全部模型并写入本地画像，无需 API Key。

use std::io::IsTerminal;

use langbridge_core::evidence::{calibrate_from_evidence, list_evidence_models};
use langbridge_core::profile_store::profile_from_calibration;

use crate::args::ParsedArgs;
use crate::format::green;
use crate::store::{write_profiles, CliError};

pub fn setup_command(args: &ParsedArgs) -> Result<i32, CliError> {
    let as_json = args.flag_true("json") || !std::io::stdout().is_terminal();
    let ids = list_evidence_models();
    if ids.is_empty() {
        return Err(CliError::new(2, "没有内置证据数据，无法批量校准"));
    }

    let mut profiles = Vec::new();
    let mut failed: Vec<(String, String)> = Vec::new();
    for id in &ids {
        match calibrate_from_evidence(id) {
            Ok(calibration) => {
                profiles.push(profile_from_calibration(id, calibration, None));
            }
            Err(err) => failed.push((id.clone(), err.message)),
        }
    }
    if profiles.is_empty() {
        return Err(CliError::new(
            2,
            format!(
                "全部模型校准失败：{}",
                failed
                    .iter()
                    .map(|(id, msg)| format!("{}（{}）", id, msg))
                    .collect::<Vec<_>>()
                    .join("；")
            ),
        ));
    }

    let paths = write_profiles(&profiles)?;
    let written: Vec<(String, f64, String)> = profiles
        .iter()
        .zip(paths)
        .map(|(profile, path)| {
            (
                profile.model.model_id.clone(),
                profile.measurement.effective_ratio,
                path.display().to_string(),
            )
        })
        .collect();

    if as_json {
        let written_json: Vec<serde_json::Value> = written
            .iter()
            .map(|(id, ratio, path)| {
                serde_json::json!({ "model_id": id, "ratio": ratio, "path": path })
            })
            .collect();
        let failed_json: Vec<serde_json::Value> = failed
            .iter()
            .map(|(id, msg)| serde_json::json!({ "model_id": id, "message": msg }))
            .collect();
        let payload = serde_json::json!({ "written": written_json, "failed": failed_json });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
    } else {
        for (id, ratio, path) in &written {
            println!(
                "{} {} · 比值 {:.4} · {}",
                green("已校准"),
                id,
                ratio,
                path
            );
        }
        for (id, msg) in &failed {
            eprintln!("失败 {}：{}", id, msg);
        }
        println!(
            "共写入 {} 份画像{}",
            written.len(),
            if failed.is_empty() {
                String::new()
            } else {
                format!("，{} 份失败", failed.len())
            }
        );
    }
    Ok(0)
}
