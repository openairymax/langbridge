//! 输出后处理：标签解析、漂移检测、术语替换与空白归一。
//! 流式响应只做统计，不改写内容。

use std::sync::OnceLock;

use regex::Regex;

use crate::signals::{purity_for, strip_code};
use crate::types::Lang;

#[derive(Debug, Clone)]
pub struct ParsedOutput {
    pub thinking: String,
    pub answer: String,
    pub had_tags: bool,
}

fn thinking_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?is)<thinking>([\s\S]*?)</thinking>").unwrap())
}

fn answer_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?is)<answer>([\s\S]*?)(?:</answer>|$)").unwrap())
}

/// 双标签解析，无标签时全文作为 answer。
pub fn parse_output(raw: &str) -> ParsedOutput {
    let thinking_blocks: Vec<String> = thinking_re()
        .captures_iter(raw)
        .map(|c| c[1].to_string())
        .collect();
    let answer_blocks: Vec<String> = answer_re()
        .captures_iter(raw)
        .map(|c| c[1].to_string())
        .collect();
    let had_tags = !thinking_blocks.is_empty() || !answer_blocks.is_empty();
    let answer = if !answer_blocks.is_empty() {
        answer_blocks.join("\n").trim().to_string()
    } else {
        thinking_re().replace_all(raw, "").trim().to_string()
    };
    ParsedOutput {
        thinking: thinking_blocks.join("\n").trim().to_string(),
        answer,
        had_tags,
    }
}

#[derive(Debug, Clone, Copy)]
pub struct DriftResult {
    pub purity: f64,
    pub drifted: bool,
    pub threshold: f64,
}

/// 漂移阈值：期望语言纯度低于该值时判定漂移，仅记录不阻断。
pub const DRIFT_THRESHOLD: f64 = 0.6;

pub fn detect_drift(text: &str, expected: Lang, threshold: f64) -> DriftResult {
    let purity = purity_for(text, expected);
    DriftResult {
        purity: (purity * 100.0).round() / 100.0,
        drifted: purity < threshold,
        threshold,
    }
}

const TERMS: &[(&str, &str)] = &[
    (r"(?i)\bgradient descent\b", "梯度下降"),
    (r"(?i)\bmachine learning\b", "机器学习"),
    (r"(?i)\bneural network\b", "神经网络"),
    (r"(?i)\blarge language model\b", "大语言模型"),
    (r"(?i)\btokenizer\b", "分词器"),
    (r"(?i)\bfine-?tuning\b", "微调"),
    (r"(?i)\bembedding\b", "嵌入"),
    (r"(?i)\blatency\b", "延迟"),
    (r"(?i)\bthroughput\b", "吞吐量"),
];

fn code_segment_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(r"(?s)```[\s\S]*?```|`[^`\n]*`").unwrap())
}

fn split_code_segments(text: &str) -> Vec<(bool, String)> {
    let mut parts = Vec::new();
    let mut last = 0usize;
    for m in code_segment_re().find_iter(text) {
        if m.start() > last {
            parts.push((false, text[last..m.start()].to_string()));
        }
        parts.push((true, m.as_str().to_string()));
        last = m.end();
    }
    if last < text.len() {
        parts.push((false, text[last..].to_string()));
    }
    parts
}

/// 术语一致性替换，只处理正文，跳过代码块与行内代码。
pub fn apply_terminology(text: &str) -> (String, usize) {
    let mut applied = 0usize;
    let out = split_code_segments(text)
        .into_iter()
        .map(|(is_code, seg)| {
            if is_code {
                return seg;
            }
            let mut s = seg;
            for (pattern, zh) in TERMS {
                let re = Regex::new(pattern).expect("术语正则应可编译");
                let count = re.find_iter(&s).count();
                if count > 0 {
                    applied += count;
                    s = re.replace_all(&s, *zh).into_owned();
                }
            }
            s
        })
        .collect::<Vec<_>>()
        .join("");
    (out, applied)
}

fn polish_text(text: &str) -> String {
    let crlf = Regex::new(r"\r\n").unwrap();
    let trailing = Regex::new(r"[ \t]+\n").unwrap();
    let blanks = Regex::new(r"\n{3,}").unwrap();
    let spaces = Regex::new(r"[ \t]{2,}").unwrap();
    let cjk_space = Regex::new(r"([\u{4e00}-\u{9fff}])\s+([\u{4e00}-\u{9fff}])").unwrap();
    let cjk_punct_before = Regex::new(r"([\u{4e00}-\u{9fff}])\s+([，。！？；：、])").unwrap();
    let cjk_punct_after = Regex::new(r"([，。！？；：、])\s+([\u{4e00}-\u{9fff}])").unwrap();
    let dup_de = Regex::new("的的").unwrap();
    let dup_le = Regex::new("了了").unwrap();
    let lets = Regex::new("让我们").unwrap();

    let mut s = crlf.replace_all(text, "\n").into_owned();
    s = trailing.replace_all(&s, "\n").into_owned();
    s = blanks.replace_all(&s, "\n\n").into_owned();
    s = spaces.replace_all(&s, " ").into_owned();
    s = cjk_space.replace_all(&s, "$1$2").into_owned();
    s = cjk_punct_before.replace_all(&s, "$1$2").into_owned();
    s = cjk_punct_after.replace_all(&s, "$1$2").into_owned();
    s = dup_de.replace_all(&s, "的").into_owned();
    s = dup_le.replace_all(&s, "了").into_owned();
    lets.replace_all(&s, "我们").into_owned()
}

/// 文本润色：归一空白、清理中英文间距与重复虚词。仅在非流式输出上执行。
pub fn polish(text: &str) -> String {
    split_code_segments(text)
        .into_iter()
        .map(|(is_code, seg)| if is_code { seg } else { polish_text(&seg) })
        .collect::<Vec<_>>()
        .join("")
        .trim()
        .to_string()
}

#[derive(Debug, Clone)]
pub struct PostprocessOptions {
    pub expected_lang: Lang,
    /// 流式响应一律不改写，只统计
    pub streaming: bool,
}

#[derive(Debug, Clone)]
pub struct PostprocessResult {
    pub answer: String,
    pub thinking: String,
    pub had_tags: bool,
    pub purity: f64,
    pub drifted: bool,
    pub terminology_applied: usize,
    pub polished: bool,
}

pub fn postprocess(raw: &str, opts: &PostprocessOptions) -> PostprocessResult {
    let parsed = parse_output(raw);
    // 漂移检测在改写前采样，避免术语替换与润色改变语言纯度
    let drift = detect_drift(&parsed.answer, opts.expected_lang, DRIFT_THRESHOLD);
    let mut answer = parsed.answer;
    let mut terminology_applied = 0usize;
    let mut polished = false;
    if !opts.streaming {
        // 术语表把英文术语替换为中文，仅在期望输出为中文时应用
        if opts.expected_lang == Lang::Zh {
            let (text, applied) = apply_terminology(&answer);
            answer = text;
            terminology_applied = applied;
        }
        let before = answer.clone();
        answer = polish(&answer);
        polished = before != answer;
    }
    PostprocessResult {
        answer,
        thinking: parsed.thinking,
        had_tags: parsed.had_tags,
        purity: drift.purity,
        drifted: drift.drifted,
        terminology_applied,
        polished,
    }
}

/// 仅统计，不修改内容。
pub fn inspect_only(text: &str, expected_lang: Lang) -> DriftResult {
    detect_drift(&strip_code(text), expected_lang, DRIFT_THRESHOLD)
}
