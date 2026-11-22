// proxy：启动本机 OpenAI 兼容代理（HTTP/1.1 + 线程）

use std::path::Path;

use langbridge_core::profiles::meta_for;
use langbridge_core::router::{route, RouteInput};
use langbridge_core::savings::estimate_r;
use langbridge_core::signals::extract_signals;
use langbridge_core::now_ms;

use crate::args::ParsedArgs;
use crate::format::{green, js_num, lang_code, line, yellow};
use crate::proxy::{start_proxy, target_url, ProxyOptions};
use crate::store::{read_profile, read_profile_file, CliError};

const DEFAULT_UPSTREAM: &str = "https://api.deepseek.com/v1";
const DEFAULT_UPSTREAM_TIMEOUT_MS: u64 = 120_000;

fn list_arg(args: &ParsedArgs, name: &str) -> Vec<String> {
    args.str(name)
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

pub fn proxy_command(args: &ParsedArgs) -> Result<i32, CliError> {
    let upstream = args
        .str("upstream")
        .map(|s| s.to_string())
        .or_else(|| std::env::var("LANGBRIDGE_UPSTREAM").ok())
        .unwrap_or_else(|| DEFAULT_UPSTREAM.to_string());
    if target_url(&upstream).is_none() {
        return Err(CliError::new(
            1,
            "--upstream 需要合法的 http(s) 地址，例如 https://api.deepseek.com/v1",
        ));
    }

    let port: u16 = match args.str("port") {
        Some(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() || !trimmed.chars().all(|c| c.is_ascii_digit()) {
                return Err(CliError::new(1, "--port 必须是 1–65535 的整数"));
            }
            match trimmed.parse::<u16>() {
                Ok(p) if p > 0 => p,
                _ => return Err(CliError::new(1, "--port 必须是 1–65535 的整数")),
            }
        }
        None => 8787,
    };
    let host = args.str("host").unwrap_or("127.0.0.1").to_string();

    let profile_path = args.str("profile").map(|s| s.to_string());
    let explicit = match &profile_path {
        Some(p) => Some(read_profile_file(Path::new(p))?),
        None => None,
    };

    if args.has("dry-run") {
        let text = args.str("text").unwrap_or("解释一下这段 Rust 代码").to_string();
        let model_id = args
            .str("model")
            .map(|s| s.to_string())
            .or_else(|| explicit.as_ref().map(|p| p.model.model_id.clone()))
            .unwrap_or_else(|| "deepseek-reasoner".to_string());
        let profile = explicit;
        let model = meta_for(
            &model_id,
            profile.as_ref().map(|p| p.measurement.effective_ratio),
        );
        let signals = extract_signals(&text, "");
        let r = estimate_r(signals.task, &model);
        let decision = route(&RouteInput {
            signals: &signals,
            model: &model,
            profile: profile.as_ref(),
            r,
            streaming: false,
            user_system_prompt: None,
            now_ms: now_ms(),
        });
        println!(
            "{}",
            line(
                "注入决策",
                &format!(
                    "{} · {}",
                    lang_code(decision.routing.decided_lang),
                    lang_code(decision.routing.output_lang)
                )
            )
        );
        println!(
            "{}",
            line(
                "注入状态",
                &format!(
                    "{} · {} tokens",
                    crate::format::injection_state_code(decision.injection.state),
                    js_num(decision.injection.prompt_tokens + decision.injection.tag_tokens)
                )
            )
        );
        if !decision.injection.system_prompt.is_empty() {
            println!("{}", line("注入内容", &decision.injection.system_prompt));
        }
        return Ok(0);
    }

    let model_id = args
        .str("model")
        .map(|s| s.to_string())
        .or_else(|| explicit.as_ref().map(|p| p.model.model_id.clone()))
        .unwrap_or_else(|| "deepseek-reasoner".to_string());
    if let Some(p) = &explicit {
        if p.model.model_id != model_id {
            eprintln!(
                "{}",
                yellow(&format!(
                    "提示：画像属于 {}，本次请求会按 {} 路由",
                    p.model.model_id, model_id
                ))
            );
        }
    }
    let profile = match explicit {
        Some(p) => Some(p),
        None => read_profile(&model_id)?,
    };

    let api_key = args
        .str("api-key")
        .map(|s| s.to_string())
        .or_else(|| std::env::var("LANGBRIDGE_API_KEY").ok());
    let timeout_raw = args.has("upstream-timeout-ms");
    let upstream_timeout_ms = if timeout_raw {
        let parsed = args.num("upstream-timeout-ms");
        match parsed {
            Some(n) if n.fract() == 0.0 && n >= 1000.0 => n as u64,
            _ => {
                return Err(CliError::new(
                    1,
                    "--upstream-timeout-ms 必须是不小于 1000 的整数",
                ))
            }
        }
    } else {
        DEFAULT_UPSTREAM_TIMEOUT_MS
    };

    let opts = ProxyOptions {
        upstream,
        port,
        host: host.clone(),
        api_key,
        model: model_id,
        profile,
        quiet: args.has("quiet"),
        allow_origins: list_arg(args, "allow-origin"),
        allow_hosts: list_arg(args, "allow-host"),
        upstream_timeout_ms,
    };

    let running = start_proxy(opts)
        .map_err(|e| CliError::new(1, format!("内部错误：{}", e)))?;
    println!("{} {}", green("代理已启动"), running.url);
    println!("health  {}/health", running.url);
    let _ = running.handle.join();
    Ok(0)
}
