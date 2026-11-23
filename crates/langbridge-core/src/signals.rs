//! 信号提取：语言、任务类型与代码占比。

use std::sync::OnceLock;

use regex::Regex;

use crate::types::{Lang, Signals, TaskType};

/// 语言分析窗口：前 4000 个 UTF-16 编码单元。
pub const ANALYSIS_WINDOW: usize = 4000;

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("正则应在编译期通过"))
}

macro_rules! lazy_re {
    ($name:ident, $pat:expr) => {
        fn $name() -> &'static Regex {
            static CELL: OnceLock<Regex> = OnceLock::new();
            re(&CELL, $pat)
        }
    };
}

lazy_re!(fence_re, r"(?s)```.*?```");
lazy_re!(inline_code_re, r"`[^`\n]*`");
lazy_re!(url_re, r"https?://\S+");
lazy_re!(cjk_re, r"[\u{4e00}-\u{9fff}]");
lazy_re!(latin_re, r"[a-zA-Z]");

/// 按 UTF-16 编码单元截断，不会切在多字节字符中间。
pub fn slice_utf16(s: &str, max: usize) -> &str {
    let mut units = 0usize;
    for (idx, ch) in s.char_indices() {
        let len = ch.len_utf16();
        if units + len > max {
            return &s[..idx];
        }
        units += len;
    }
    s
}

pub fn utf16_len(s: &str) -> usize {
    s.encode_utf16().count()
}

pub fn strip_code(text: &str) -> String {
    let a = fence_re().replace_all(text, " ");
    let b = inline_code_re().replace_all(&a, " ");
    url_re().replace_all(&b, " ").into_owned()
}

fn count(text: &str, re: &Regex) -> usize {
    re.find_iter(text).count()
}

fn clamp(v: f64, lo: f64, hi: f64) -> f64 {
    v.max(lo).min(hi)
}

/// 四舍五入。负数按远离零方向取整。
pub fn round_js(v: f64) -> f64 {
    v.round()
}

pub fn round2(v: f64) -> f64 {
    (v * 100.0).round() / 100.0
}

#[derive(Debug, Clone)]
pub struct LanguageDetection {
    pub lang: Lang,
    pub confidence: f64,
    pub mixed: bool,
    pub zh_mass: f64,
    pub en_mass: f64,
    pub zh_share: f64,
}

pub fn detect_language(raw: &str) -> LanguageDetection {
    let text = strip_code(slice_utf16(raw, ANALYSIS_WINDOW));
    let cjk_chars = count(&text, cjk_re());
    let latin_chars = count(&text, latin_re());
    let zh_mass = cjk_chars as f64;
    let en_mass = latin_chars as f64 / 4.5;
    let total = zh_mass + en_mass;

    if total < 1.0 || (cjk_chars < 2 && latin_chars < 2) {
        return LanguageDetection {
            lang: Lang::En,
            confidence: 0.3,
            mixed: false,
            zh_mass,
            en_mass,
            zh_share: 0.0,
        };
    }

    let zh_share = zh_mass / (zh_mass + en_mass + 1e-6);
    let lang = if zh_share >= 0.2 && cjk_chars >= 2 {
        Lang::Zh
    } else {
        Lang::En
    };
    let mixed = total > 0.0 && zh_mass / total >= 0.2 && en_mass / total >= 0.2;
    let confidence = clamp(
        0.35 + (zh_share - 0.5).abs() * 1.2 + (total / 200.0).min(0.2),
        0.3,
        0.98,
    );
    LanguageDetection {
        lang,
        confidence,
        mixed,
        zh_mass,
        en_mass,
        zh_share,
    }
}

lazy_re!(code_fence, r"```");
lazy_re!(
    code_kw,
    r"\b(function|def|class|import|export|return|async|await)\s+[\w$({]"
);
lazy_re!(code_var, r"\b(const|let|var)\s+\w+\s*=");
lazy_re!(code_arrow, r"=>\s*[{(]");
lazy_re!(code_log, r"\b(console\.log|print)\s*\(");
lazy_re!(code_include, r"#include\s*<");
lazy_re!(
    code_sql,
    r"(?i)\b(SELECT|INSERT|UPDATE|DELETE)\b[\s\S]{0,40}\bFROM\b"
);
lazy_re!(code_zh_fn, r"写(一)?(个|段)?(函数|脚本|程序|代码|算法)");
lazy_re!(code_zh_sort, r"(快速|冒泡|归并|选择|插入)排序");
lazy_re!(
    code_zh_build,
    r"(实现|写)(一)?(个|下)?(快排|二分|递归|爬虫|接口|组件|页面|服务)"
);
lazy_re!(
    code_lang,
    r"(Python|Java(Script)?|TypeScript|C\+\+|Golang|Rust|SQL)\b"
);

lazy_re!(math_expr, r"[0-9]\s*[+\-*/^]\s*[0-9]");
lazy_re!(
    math_en,
    r"(?i)\b(prove|solve|derivative|integral|matrix|prime)\b"
);
lazy_re!(math_symbol, r"[∑∫√≠≤≥]");
lazy_re!(
    math_zh,
    r"证明|求解|方程|微积分|导数|积分|整除|质数|素数|几何|概率"
);
lazy_re!(
    math_count,
    r"[0-9]+\s*(个|次|只|条|人|数).{0,4}(等于|是|为)"
);

lazy_re!(
    culture_zh,
    r"成语|典故|诗词|唐诗|宋词|文言文|古文|对联|歇后语|俗语|诗经|论语|道德经"
);
lazy_re!(culture_book, r"红楼梦|三国演义|水浒传|西游记|史记|资治通鉴");
lazy_re!(
    culture_write,
    r"仿写.{0,4}(古|诗)|写出.{0,4}诗|翻译.{0,4}文言"
);

lazy_re!(
    policy_zh,
    r"政策|法规|法律|条例|规定|办法|细则|合规|监管|公文|红头文件"
);
lazy_re!(
    policy_en,
    r"(?i)\b(policy|regulation|compliance|statute|ordinance)\b"
);

lazy_re!(
    hint_kw,
    r"\b(function|def|class|import|export|const|let|var)\b"
);
lazy_re!(hint_semi, r"(?m)[;{}]\s*$");

fn score(text: &str, patterns: &[(&Regex, u32)]) -> u32 {
    patterns
        .iter()
        .filter(|(re, _)| re.is_match(text))
        .map(|(_, w)| *w)
        .sum()
}

pub struct TaskDetection {
    pub task: TaskType,
    pub has_code: bool,
    pub has_math: bool,
}

pub fn detect_task(raw: &str) -> TaskDetection {
    let text = slice_utf16(raw, ANALYSIS_WINDOW);
    let has_code = [
        code_fence(),
        hint_kw(),
        code_arrow(),
        code_log(),
        hint_semi(),
        code_zh_fn(),
        code_zh_sort(),
        code_lang(),
    ]
    .iter()
    .any(|re| re.is_match(text));
    let has_math = [
        math_expr(),
        math_en(),
        math_symbol(),
        math_zh(),
        math_count(),
    ]
    .iter()
    .any(|re| re.is_match(text));

    let code = score(
        text,
        &[
            (code_fence(), 5),
            (code_kw(), 2),
            (code_var(), 2),
            (code_arrow(), 2),
            (code_log(), 2),
            (code_include(), 3),
            (code_sql(), 3),
            (code_zh_fn(), 3),
            (code_zh_sort(), 3),
            (code_zh_build(), 2),
            (code_lang(), 2),
        ],
    );
    let math = score(
        text,
        &[
            (math_expr(), 2),
            (math_en(), 2),
            (math_symbol(), 2),
            (math_zh(), 2),
            (math_count(), 1),
        ],
    );
    let culture = score(
        text,
        &[(culture_zh(), 3), (culture_book(), 3), (culture_write(), 3)],
    );
    let policy = score(text, &[(policy_zh(), 3), (policy_en(), 3)]);

    let mut best = TaskType::General;
    let mut best_score = 0u32;
    for (task, s) in [
        (TaskType::Code, code),
        (TaskType::Math, math),
        (TaskType::Culture, culture),
        (TaskType::Policy, policy),
    ] {
        if s > best_score {
            best = task;
            best_score = s;
        }
    }
    if best_score == 0 {
        best = TaskType::General;
    }
    TaskDetection {
        task: best,
        has_code,
        has_math,
    }
}

pub fn estimate_tokens(text: &str) -> u32 {
    let cjk_chars = count(text, cjk_re());
    let latin_chars = count(text, latin_re());
    let v = cjk_chars as f64 * 0.9 + latin_chars as f64 / 4.5;
    v.round().max(1.0) as u32
}

/// `history` 仅估算历史 token，不参与语言与任务判定
pub fn extract_signals(raw: &str, history: &str) -> Signals {
    let det = detect_language(raw);
    let task = detect_task(raw);
    Signals {
        lang: det.lang,
        lang_confidence: round2(det.confidence),
        mixed: det.mixed,
        zh_mass: round2(det.zh_mass),
        en_mass: round2(det.en_mass),
        task: task.task,
        has_code: task.has_code,
        has_math: task.has_math,
        input_chars: utf16_len(raw),
        input_tokens_est: estimate_tokens(raw),
        history_tokens_est: if history.trim().is_empty() {
            0
        } else {
            estimate_tokens(history)
        },
    }
}

pub fn zh_purity(text: &str) -> f64 {
    let clean = strip_code(text);
    let cjk = count(&clean, cjk_re()) as f64;
    let latin = count(&clean, latin_re()) as f64 / 4.5;
    let total = cjk + latin;
    if total == 0.0 {
        0.0
    } else {
        cjk / total
    }
}

pub fn purity_for(text: &str, lang: Lang) -> f64 {
    let zh = zh_purity(text);
    match lang {
        Lang::Zh => zh,
        Lang::En => 1.0 - zh,
    }
}
