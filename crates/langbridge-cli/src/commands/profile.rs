//! profile 子命令：列出、查看、导入、导出、校验、删除本地画像，或输出画像目录。

use std::fs;
use std::io::IsTerminal;
use std::path::Path;

use langbridge_core::profile_store::{parse_profile_file, serialize_profile};
use langbridge_core::now_ms;

use crate::args::ParsedArgs;
use crate::format::{green, print_profile, source_code, yellow};
use crate::store::{
    export_path_for, home_dir, list_profile_files, profile_path, read_profile, read_profile_file,
    remove_profile, write_profile, CliError,
};

fn source_json(source: langbridge_core::ProfileSource) -> String {
    serde_json::to_value(source)
        .ok()
        .and_then(|v| v.as_str().map(|s| s.to_string()))
        .unwrap_or_else(|| source_code(source).to_string())
}

pub fn profile_command(args: &ParsedArgs) -> Result<i32, CliError> {
    let sub = args
        .positional
        .first()
        .cloned()
        .unwrap_or_else(|| "list".to_string());
    let as_json = args.flag_true("json") || !std::io::stdout().is_terminal();

    if sub == "list" {
        let files = list_profile_files();
        if as_json {
            let mut items: Vec<serde_json::Value> = Vec::new();
            for f in &files {
                let p = read_profile_file(f)?;
                items.push(serde_json::json!({
                    "model_id": p.model.model_id,
                    "ratio": p.measurement.effective_ratio,
                    "confidence": p.measurement.confidence,
                    "source": source_json(p.provenance.source),
                    "measured_at": p.measurement.measured_at,
                    "path": f.display().to_string(),
                }));
            }
            println!(
                "{}",
                serde_json::to_string_pretty(&items).unwrap_or_default()
            );
            return Ok(0);
        }
        if files.is_empty() {
            println!(
                "{}",
                yellow("暂无本地画像。先运行：langbridge calibrate --model deepseek-reasoner")
            );
            return Ok(0);
        }
        for f in &files {
            let p = read_profile_file(f)?;
            let id = &p.model.model_id;
            let padded = if id.chars().count() >= 24 {
                id.clone()
            } else {
                format!("{}{}", id, " ".repeat(24 - id.chars().count()))
            };
            println!(
                "{} ratio {:.3}  conf {:.2}  {}  {}",
                padded,
                p.measurement.effective_ratio,
                p.measurement.confidence,
                source_code(p.provenance.source),
                f.display()
            );
        }
        return Ok(0);
    }

    if sub == "path" {
        println!("{}", home_dir().display());
        return Ok(0);
    }

    if sub == "show" {
        let model_id = args
            .str("model")
            .map(|s| s.to_string())
            .or_else(|| args.positional.get(1).cloned());
        let Some(model_id) = model_id else {
            return Err(CliError::new(1, "用法：langbridge profile show <model>"));
        };
        let Some(profile) = read_profile(&model_id)? else {
            return Err(CliError::new(2, format!("没有找到 {} 的画像", model_id)));
        };
        if as_json {
            println!("{}", serialize_profile(&profile));
        } else {
            print_profile(&profile);
        }
        return Ok(0);
    }

    if sub == "validate" || sub == "import" {
        let file = args
            .str("file")
            .map(|s| s.to_string())
            .or_else(|| args.positional.get(1).cloned());
        let Some(file) = file else {
            let usage = if sub == "validate" {
                "用法：langbridge profile validate <file>"
            } else {
                "用法：langbridge profile import <file>"
            };
            return Err(CliError::new(1, usage));
        };
        let raw = fs::read_to_string(&file)
            .map_err(|_| CliError::new(2, format!("无法读取文件：{}", file)))?;
        let parsed = parse_profile_file(&raw, now_ms());
        if !parsed.ok || parsed.profile.is_none() {
            let detail = parsed
                .errors
                .iter()
                .map(|e| format!("  {}: {}", e.path, e.message))
                .collect::<Vec<_>>()
                .join("\n");
            return Err(CliError::new(4, format!("画像校验失败：\n{}", detail)));
        }
        let profile = parsed.profile.unwrap();
        if sub == "validate" {
            if as_json {
                let payload = serde_json::json!({
                    "ok": true,
                    "model_id": profile.model.model_id,
                    "version": profile.version,
                    "ratio": profile.measurement.effective_ratio,
                    "confidence": profile.measurement.confidence,
                });
                println!(
                    "{}",
                    serde_json::to_string_pretty(&payload).unwrap_or_default()
                );
            } else {
                println!(
                    "{} {}  ratio {:.3}  conf {:.2}",
                    green("校验通过"),
                    profile.model.model_id,
                    profile.measurement.effective_ratio,
                    profile.measurement.confidence
                );
            }
            return Ok(0);
        }
        let out = write_profile(&profile, None)?;
        println!(
            "{} {} → {}",
            green("导入成功"),
            profile.model.model_id,
            out.display()
        );
        return Ok(0);
    }

    if sub == "export" {
        let model_id = args
            .str("model")
            .map(|s| s.to_string())
            .or_else(|| args.positional.get(1).cloned());
        let Some(model_id) = model_id else {
            return Err(CliError::new(
                1,
                "用法：langbridge profile export <model> [--out file]",
            ));
        };
        let Some(profile) = read_profile(&model_id)? else {
            return Err(CliError::new(2, format!("没有找到 {} 的画像", model_id)));
        };
        let out = args
            .str("out")
            .map(|s| s.to_string())
            .unwrap_or_else(|| export_path_for(&profile).display().to_string());
        if let Some(parent) = Path::new(&out).parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)
                    .map_err(|e| CliError::new(2, format!("无法创建目录：{}（{}）", parent.display(), e)))?;
            }
        }
        fs::write(&out, serialize_profile(&profile))
            .map_err(|e| CliError::new(2, format!("无法写入文件：{}（{}）", out, e)))?;
        println!("{} {}", green("已导出"), out);
        return Ok(0);
    }

    if sub == "remove" {
        let model_id = args
            .str("model")
            .map(|s| s.to_string())
            .or_else(|| args.positional.get(1).cloned());
        let Some(model_id) = model_id else {
            return Err(CliError::new(1, "用法：langbridge profile remove <model>"));
        };
        let removed = remove_profile(&model_id)?;
        if removed {
            println!("{} {}", green("已删除"), profile_path(&model_id).display());
        } else {
            println!("{}", yellow("没有该画像"));
        }
        return Ok(0);
    }

    Err(CliError::new(1, format!("未知子命令：profile {}", sub)))
}
