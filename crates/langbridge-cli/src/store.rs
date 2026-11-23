//! 本地画像存储，默认位于 ~/.langbridge/<model>.json，可由 LANGBRIDGE_HOME 覆盖。
//!
//! 写入先落 .tmp 再 rename，避免中断留下半截文件；容量只统计 HOME 顶层的
//! *.json 文件。

use std::collections::HashSet;
use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use langbridge_core::profile_store::{parse_profile_file, profile_file_name, serialize_profile};
use langbridge_core::profiles::assert_within_capacity;
use langbridge_core::{now_ms, ModelProfile};
use regex::Regex;

use crate::args::js_parse_int;

/// 画像数量默认上限，可由 LANGBRIDGE_PROFILE_MAX 覆盖。
pub const PROFILE_CAPACITY_DEFAULT: i64 = 8;

#[derive(Debug, Clone)]
pub struct CliError {
    pub code: i32,
    pub message: String,
}

impl CliError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

pub fn profile_capacity() -> i64 {
    match env::var("LANGBRIDGE_PROFILE_MAX") {
        Ok(raw) => match js_parse_int(&raw) {
            Some(n) if n > 0 => n,
            _ => PROFILE_CAPACITY_DEFAULT,
        },
        Err(_) => PROFILE_CAPACITY_DEFAULT,
    }
}

fn absolute(p: &str) -> PathBuf {
    let pb = PathBuf::from(p);
    if pb.is_absolute() {
        pb
    } else {
        std::path::absolute(&pb).unwrap_or(pb)
    }
}

pub fn home_dir() -> PathBuf {
    match env::var("LANGBRIDGE_HOME") {
        Ok(h) if !h.is_empty() => absolute(&h),
        _ => {
            let base = env::var("HOME").map(PathBuf::from).unwrap_or_default();
            base.join(".langbridge")
        }
    }
}

pub fn safe_model_id(model_id: &str) -> String {
    let re = Regex::new(r"[^A-Za-z0-9_\.-]+").unwrap();
    re.replace_all(model_id, "_").into_owned()
}

pub fn profile_path(model_id: &str) -> PathBuf {
    home_dir().join(format!("{}.json", safe_model_id(model_id)))
}

pub fn ensure_home() -> Result<PathBuf, CliError> {
    let dir = home_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| CliError::new(2, format!("无法创建画像目录：{}（{}）", dir.display(), e)))?;
    Ok(dir)
}

/// 读取并校验画像。不可读返回 2，字段非法返回 4。
pub fn read_profile_file(path: &Path) -> Result<ModelProfile, CliError> {
    let text = fs::read_to_string(path)
        .map_err(|_| CliError::new(2, format!("无法读取画像文件：{}", path.display())))?;
    let parsed = parse_profile_file(&text, now_ms());
    if !parsed.ok || parsed.profile.is_none() {
        let detail = parsed
            .errors
            .iter()
            .map(|e| format!("{}: {}", e.path, e.message))
            .collect::<Vec<_>>()
            .join("; ");
        let detail = if detail.is_empty() {
            "未知错误".to_string()
        } else {
            detail
        };
        return Err(CliError::new(4, format!("画像校验失败：{}", detail)));
    }
    Ok(parsed.profile.unwrap())
}

pub fn read_profile(model_id: &str) -> Result<Option<ModelProfile>, CliError> {
    let path = profile_path(model_id);
    if !path.exists() {
        return Ok(None);
    }
    read_profile_file(&path).map(Some)
}

/// 写入前按目标容量整体校验，任一越界则整批拒绝。
pub fn assert_profile_capacity(paths: &[PathBuf]) -> Result<(), CliError> {
    let unique: HashSet<&PathBuf> = paths.iter().collect();
    let missing = unique.iter().filter(|p| !p.exists()).count() as i64;
    let current = list_profile_files().len() as i64;
    let capacity = profile_capacity();
    if let Err(err) = assert_within_capacity((current + missing) as u64, capacity as u64) {
        return Err(CliError::new(
            2,
            format!(
                "{}；当前 {} 个画像，本次新增 {} 个，上限 {} 个。可用 `langbridge profile remove <model-id>` 释放名额。",
                err.message, current, missing, capacity
            ),
        ));
    }
    Ok(())
}

/// 原子写入：先写 .tmp 再 rename，避免中断留下半截文件。
pub fn write_profile(profile: &ModelProfile, path: Option<&Path>) -> Result<PathBuf, CliError> {
    ensure_home()?;
    let target = match path {
        Some(p) => p.to_path_buf(),
        None => profile_path(&profile.model.model_id),
    };
    assert_profile_capacity(std::slice::from_ref(&target))?;
    let tmp = PathBuf::from(format!("{}.tmp", target.display()));
    fs::write(&tmp, serialize_profile(profile))
        .map_err(|e| CliError::new(2, format!("写入画像失败：{}（{}）", tmp.display(), e)))?;
    fs::rename(&tmp, &target)
        .map_err(|e| CliError::new(2, format!("写入画像失败：{}（{}）", target.display(), e)))?;
    Ok(target)
}

/// 批量写入：先整体容量校验，任一条越界则整批拒绝。
pub fn write_profiles(profiles: &[ModelProfile]) -> Result<Vec<PathBuf>, CliError> {
    ensure_home()?;
    let paths: Vec<PathBuf> = profiles
        .iter()
        .map(|p| profile_path(&p.model.model_id))
        .collect();
    assert_profile_capacity(&paths)?;
    for (i, profile) in profiles.iter().enumerate() {
        let tmp = PathBuf::from(format!("{}.tmp", paths[i].display()));
        fs::write(&tmp, serialize_profile(profile))
            .map_err(|e| CliError::new(2, format!("写入画像失败：{}（{}）", tmp.display(), e)))?;
        fs::rename(&tmp, &paths[i])
            .map_err(|e| CliError::new(2, format!("写入画像失败：{}（{}）", paths[i].display(), e)))?;
    }
    Ok(paths)
}

/// 仅统计 HOME 顶层的 *.json 画像文件，不递归子目录。
pub fn list_profile_files() -> Vec<PathBuf> {
    let dir = home_dir();
    let entries = match fs::read_dir(&dir) {
        Ok(e) => e,
        Err(_) => return Vec::new(),
    };
    let mut out: Vec<PathBuf> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "json") {
            out.push(path);
        }
    }
    out.sort();
    out
}

pub fn remove_profile(model_id: &str) -> Result<bool, CliError> {
    let path = profile_path(model_id);
    if !path.exists() {
        return Ok(false);
    }
    fs::remove_file(&path)
        .map_err(|e| CliError::new(2, format!("无法删除画像：{}（{}）", path.display(), e)))?;
    Ok(true)
}

/// 默认导出到 HOME/exports/，避免占用画像容量名额。
pub fn export_path_for(profile: &ModelProfile) -> PathBuf {
    home_dir().join("exports").join(profile_file_name(profile))
}
