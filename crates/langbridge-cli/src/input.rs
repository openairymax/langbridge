// CLI 输入读取：--text / --file / 管道 stdin 三种来源，供 decide / inject / guard 共用

use std::io::{IsTerminal, Read};

use crate::args::ParsedArgs;
use crate::store::CliError;

pub fn read_stdin() -> String {
    let mut buf = String::new();
    let _ = std::io::stdin().read_to_string(&mut buf);
    buf
}

pub fn read_input(args: &ParsedArgs) -> Result<String, CliError> {
    if let Some(text) = args.str("text") {
        return Ok(text.to_string());
    }
    if let Some(file) = args.str("file") {
        if !file.is_empty() {
            if file == "-" {
                return Ok(read_stdin());
            }
            return std::fs::read_to_string(file)
                .map_err(|_| CliError::new(1, format!("无法读取输入文件：{}", file)));
        }
    }
    if !std::io::stdin().is_terminal() {
        let piped = read_stdin();
        if !piped.trim().is_empty() {
            return Ok(piped);
        }
    }
    Err(CliError::new(
        1,
        "缺少输入：使用 --text / --file / 管道 stdin",
    ))
}
