// CLI 入口：命令分发、全局参数、退出码与统一错误出口

mod args;
mod commands;
mod format;
mod input;
mod probe;
mod proxy;
mod store;
mod util;

use std::path::PathBuf;

use store::CliError;

const VERSION: &str = "1.0.0";

const GLOBAL_FLAGS: &[&str] = &["json", "quiet", "no-color", "home", "help", "version"];

const HELP: &str = "LangBridge CLI 1.0.0

用法：
  langbridge <command> [options]

命令：
  doctor                         环境与证据自检
  calibrate --model <id>         校准并保存画像（默认 --method builtin，零 Key）
      --method builtin|api       （别名 --via）
      --n <120>                  基准语料规模，须为 120 的整数倍
      --rerun                    再测一次并与本地画像比较偏差
      --base-url <url>            API 探测必填
      --api-key <key> | --key-env <name>   Key 只存内存
      --out <file>               自定义画像输出路径
      --dry-run                  只计算不写盘
  decide --model <id>            对一条输入做四层路由与净收益决策
      --text <string> | --file <path> | stdin
      --task auto|code|math|culture|policy|general
      --r <tokens>               覆盖预期推理规模（别名 --R）
      --profile <file>           使用指定画像文件
      --streaming                按流式协议生成注入
      --system-prompt <text>     模拟用户自带 system prompt
  inject --model <id>            单次注入：只输出语言约束注入计划（脚本编排用）
      --text <string> | --file <path> | stdin
      其余参数同 decide；例：echo \"解释闭包\" | langbridge inject --model deepseek-reasoner
  guard --expect zh|en           单次后处理：标签解析、漂移检测、术语一致性、意译润色
      --text <string> | --file <path> | stdin
      --streaming                只统计不改写
      例：langbridge guard --expect zh --text \"<thinking>…</thinking><answer>…</answer>\"
  setup                          一键校准全部有证据的模型并写入本地画像（零 Key）
  profile [list|show|import|export|validate|remove|path]
      langbridge profile show deepseek-reasoner
      langbridge profile validate ./langbridge.profile.json
  proxy --upstream <url>         启动本机 OpenAI 兼容代理
      --port <n>                 默认 8787
      --profile <file>           指定画像（缺省读取本地画像）
      --api-key <key>            缺省时透传请求头 Authorization
      --model <id>               请求未带 model 时的默认值
      --dry-run --text <s>       只打印注入计划，不请求上游
      --allow-origin <list>      额外允许的浏览器来源（逗号分隔；默认只放行本站与本机）
      --allow-host <list>        额外允许的 Host 头（逗号分隔）
      --upstream-timeout-ms <n>  上游超时，默认 120000
      --quiet                    关闭脱敏日志

通用：
  --json                         输出机器可读 JSON
  --home <dir>                   覆盖 ~/.langbridge
  --no-color                     关闭颜色
  --version, -v                  版本号
  --help, -h                     帮助

退出码：0 成功（含「不建议切换」）/ 1 参数或用法 / 2 数据不可用 / 3 上游或授权 / 4 校验失败
";

fn command_flags(command: &str) -> Option<&'static [&'static str]> {
    match command {
        "doctor" => Some(&[]),
        "calibrate" => Some(&[
            "model",
            "method",
            "via",
            "n",
            "rerun",
            "out",
            "dry-run",
            "base-url",
            "api-key",
            "key-env",
            "timeout",
        ]),
        "decide" | "inject" => Some(&[
            "model",
            "text",
            "file",
            "task",
            "r",
            "R",
            "profile",
            "streaming",
            "system-prompt",
        ]),
        "guard" => Some(&["text", "file", "expect", "streaming"]),
        "setup" => Some(&[]),
        "profile" => Some(&["model", "file", "out"]),
        "proxy" => Some(&[
            "upstream",
            "port",
            "host",
            "api-key",
            "model",
            "profile",
            "dry-run",
            "text",
            "quiet",
            "allow-origin",
            "allow-host",
            "upstream-timeout-ms",
        ]),
        _ => None,
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

fn run(argv: &[String]) -> i32 {
    if argv.is_empty() || argv.iter().any(|a| a == "--help" || a == "-h") {
        println!("{}", HELP);
        return 0;
    }
    if argv.iter().any(|a| a == "--version" || a == "-v") {
        println!("{}", VERSION);
        return 0;
    }

    let command = argv[0].as_str();
    let args = args::parse_args(&argv[1..]);

    if let Some(home) = args.str("home") {
        std::env::set_var("LANGBRIDGE_HOME", absolute(home));
    }
    if args.has("no-color") {
        std::env::set_var("NO_COLOR", "1");
    }

    let Some(allowed) = command_flags(command) else {
        eprintln!("{}", format::red(&format!("未知命令：{}", command)));
        println!("{}", HELP);
        return 1;
    };

    let mut known: Vec<&str> = GLOBAL_FLAGS.to_vec();
    known.extend_from_slice(allowed);
    if let Some((name, _)) = args.flags.iter().find(|(k, _)| !known.contains(&k.as_str())) {
        eprintln!(
            "{}",
            format::red(&format!("未知参数：--{}（用 --help 查看可用参数）", name))
        );
        return 1;
    }

    let result = match command {
        "doctor" => commands::doctor::doctor_command(&args),
        "calibrate" => commands::calibrate::calibrate_command(&args),
        "decide" => commands::decide::decide_command(&args),
        "inject" => commands::inject::inject_command(&args),
        "guard" => commands::guard::guard_command(&args),
        "setup" => commands::setup::setup_command(&args),
        "profile" => commands::profile::profile_command(&args),
        "proxy" => commands::proxy::proxy_command(&args),
        _ => Ok(1),
    };

    match result {
        Ok(code) => code,
        Err(CliError { code, message }) => {
            eprintln!("{}", format::red(&message));
            code
        }
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let code = run(&argv);
    std::process::exit(code);
}
