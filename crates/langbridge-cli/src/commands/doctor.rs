//! doctor 子命令：检查运行环境、画像目录与内置证据的可用性。
//!
//! 全部通过返回 0，存在失败项返回 2。与其它子命令不同，只有显式 --json
//! 才输出 JSON，不因 stdout 非终端而自动切换。

use std::path::Path;

use langbridge_core::calibrate::calibration_usable;
use langbridge_core::evidence::{calibrate_from_evidence, evidence_meta, list_evidence_models};
use langbridge_core::profiles::models;
use serde::Serialize;

use crate::args::ParsedArgs;
use crate::format::{green, line, red, yellow};
use crate::store::{ensure_home, home_dir, list_profile_files};

#[derive(Serialize)]
struct HomeInfo {
    path: String,
    writable: bool,
}

#[derive(Serialize)]
struct EvidenceInfo {
    version: u32,
    generated_at: String,
}

#[derive(Serialize)]
struct ModelCheck {
    id: String,
    ok: bool,
    ratio: Option<f64>,
    conf: Option<f64>,
    tokenizer: Option<String>,
    error: Option<String>,
}

fn dir_writable(path: &Path) -> bool {
    let probe = path.join(format!(".langbridge-doctor-{}", std::process::id()));
    match std::fs::write(&probe, b"") {
        Ok(_) => {
            let _ = std::fs::remove_file(&probe);
            true
        }
        Err(_) => false,
    }
}

pub fn doctor_command(args: &ParsedArgs) -> Result<i32, crate::store::CliError> {
    let as_json = args.flag_true("json");
    let mut ok = true;

    let runtime_ok = true;

    let mut home_ok = true;
    if ensure_home().is_err() {
        home_ok = false;
        ok = false;
    } else if !dir_writable(&home_dir()) {
        home_ok = false;
        ok = false;
    }

    let meta = evidence_meta();
    let mut models: Vec<ModelCheck> = Vec::new();
    for id in list_evidence_models() {
        match calibrate_from_evidence(&id) {
            Ok(cal) => {
                let usable = calibration_usable(&cal);
                if !usable {
                    ok = false;
                }
                models.push(ModelCheck {
                    id,
                    ok: usable,
                    ratio: Some(cal.effective_ratio),
                    conf: Some(cal.confidence),
                    tokenizer: Some(format!(
                        "{}/{}{}",
                        cal.tokenizer.name,
                        cal.tokenizer.version,
                        if cal.tokenizer.exact == Some(false) {
                            "（同系列代算）"
                        } else {
                            ""
                        }
                    )),
                    error: if usable {
                        None
                    } else {
                        Some("未通过质量闸门".to_string())
                    },
                });
            }
            Err(err) => {
                ok = false;
                models.push(ModelCheck {
                    id,
                    ok: false,
                    ratio: None,
                    conf: None,
                    tokenizer: None,
                    error: Some(err.message),
                });
            }
        }
    }
    let files: Vec<String> = list_profile_files()
        .iter()
        .map(|p| p.display().to_string())
        .collect();

    if as_json {
        let payload = serde_json::json!({
            "ok": ok,
            "node": "rust",
            "runtime_ok": runtime_ok,
            "home": HomeInfo { path: home_dir().display().to_string(), writable: home_ok },
            "evidence": EvidenceInfo { version: meta.version, generated_at: meta.generated_at },
            "models": models,
            "registered": models_registered(),
            "profiles": files,
        });
        println!(
            "{}",
            serde_json::to_string_pretty(&payload).unwrap_or_default()
        );
        return Ok(if ok { 0 } else { 2 });
    }

    println!("{}", line("运行时", "Rust"));
    println!(
        "{}",
        line(
            "运行检查",
            &if runtime_ok {
                green("OK")
            } else {
                red("不可用")
            }
        )
    );
    if home_ok {
        println!(
            "{}",
            line(
                "画像目录",
                &format!("{} {}", home_dir().display(), green("可写"))
            )
        );
    } else {
        println!("{}", line("画像目录", &red("不可写")));
    }
    println!(
        "{}",
        line(
            "内置证据",
            &format!(
                "v{} · {} · 按各模型公开 tokenizer 计数",
                meta.version, meta.generated_at
            )
        )
    );
    println!(
        "{}",
        line(
            "证据模型",
            &models
                .iter()
                .map(|m| m.id.clone())
                .collect::<Vec<_>>()
                .join(", ")
        )
    );
    for m in &models {
        if m.ratio.is_none() || m.conf.is_none() || m.tokenizer.is_none() {
            println!(
                "{}",
                line(
                    &format!("  {}", m.id),
                    &red(m.error.as_deref().unwrap_or("不可用"))
                )
            );
        } else {
            println!(
                "{}",
                line(
                    &format!("  {}", m.id),
                    &format!(
                        "{} · ratio {:.3} · conf {:.2} · {}",
                        if m.ok { green("可用") } else { red("不可用") },
                        m.ratio.unwrap_or(0.0),
                        m.conf.unwrap_or(0.0),
                        m.tokenizer.as_deref().unwrap_or("")
                    )
                )
            );
        }
    }
    let registered = models_registered().join(", ");
    println!("{}", line("已登记模型", &registered));
    println!(
        "{}",
        line(
            "本地画像",
            &if files.is_empty() {
                yellow("无（先运行 langbridge calibrate）")
            } else {
                files.join(", ")
            }
        )
    );
    println!(
        "{}",
        line(
            "结论",
            &if ok {
                green("全部通过")
            } else {
                red("存在失败项")
            }
        )
    );
    Ok(if ok { 0 } else { 2 })
}

fn models_registered() -> Vec<String> {
    models().into_iter().map(|m| m.model_id).collect()
}
