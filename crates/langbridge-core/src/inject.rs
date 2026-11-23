//! 语言约束 system prompt 的构造与注入决策。

use std::sync::OnceLock;

use regex::Regex;

use crate::signals::estimate_tokens;
use crate::types::{
    InjectionPlan, InjectionState, Lang, ModelMeta, ModelProfile, Routing, SavingsResult, Signals,
};

#[derive(Debug, Clone)]
pub struct PromptOptions {
    pub reasoning_lang: Lang,
    pub output_lang: Lang,
    pub streaming: bool,
    /// 是否追加双标签契约，默认仅在非流式时追加
    pub use_tags: Option<bool>,
}

#[derive(Debug, Clone)]
pub struct BuiltPrompt {
    pub text: String,
    pub prompt_tokens: f64,
    pub tag_tokens: f64,
    pub uses_tags: bool,
}

const TAG_MARKERS: &str = "<thinking></thinking><answer></answer>";

fn lang_name(lang: Lang) -> &'static str {
    match lang {
        Lang::Zh => "Chinese (Simplified)",
        Lang::En => "English",
    }
}

pub fn build_system_prompt(opts: &PromptOptions) -> BuiltPrompt {
    let mut lines = vec![
        format!(
            "Reasoning language: {}. Final answer language: {}.",
            lang_name(opts.reasoning_lang),
            lang_name(opts.output_lang)
        ),
        "Do not translate code, identifiers, or quoted text.".to_string(),
        "Output that violates these language rules will be discarded.".to_string(),
    ];
    let use_tags = opts.use_tags.unwrap_or(!opts.streaming);
    if use_tags {
        lines.push(
            "Put internal reasoning in <thinking>...</thinking>, then the final reply in <answer>...</answer>."
                .to_string(),
        );
        lines.push("The user only sees the <answer> content.".to_string());
    }
    let text = lines.join("\n");
    BuiltPrompt {
        prompt_tokens: estimate_tokens(&text) as f64,
        tag_tokens: if use_tags {
            estimate_tokens(TAG_MARKERS) as f64
        } else {
            0.0
        },
        uses_tags: use_tags,
        text,
    }
}

const CONFLICT_PATTERN: &str = concat!(
    r"\b(?:answer|respond|reply|reason|think|write|output|speak|communicate|explain|summarize)\b[^.\n]{0,24}?\bin\s+(?:[A-Za-z-]+\s+){0,2}(?:",
    "English|Chinese|Mandarin|Cantonese|Japanese|Korean|French|German|Spanish|Russian|Portuguese|Italian|Arabic|Hindi",
    r")\b",
    "|",
    r"\b(?:use|using|prefer)\s+(?:only\s+)?(?:",
    "English|Chinese|Mandarin|Cantonese|Japanese|Korean|French|German|Spanish|Russian|Portuguese|Italian|Arabic|Hindi",
    r")\b\s*(?:[.,;:!?]|$|only\b|in\b|for\b|as\b|when\b|throughout\b)",
    "|",
    r"(?:请|务必|一律|全部|始终|必须|只能|仅|都)\s*(?:用|使用|以)\s*(?:",
    "简体中文|繁体中文|中文|英文|汉语|英语|简体|繁体",
    ")",
    "|",
    r"(?:用|使用|以)\s*(?:",
    "简体中文|繁体中文|中文|英文|汉语|英语|简体|繁体",
    r")\s*(?:来|去)?\s*(?:",
    "回答|回复|作答|回应|推理|思考|分析|输出|说明|解释|写|表达|总结|描述|呈现|反馈|沟通",
    ")",
    "|",
    r"(?:",
    "回答|回复|作答|回应|推理|思考|分析|输出|说明|解释|写|表达|总结|描述|呈现|反馈|沟通",
    r")\s*(?:时|请|要|必须|一律)?\s*(?:用|使用|以)\s*(?:",
    "简体中文|繁体中文|中文|英文|汉语|英语|简体|繁体",
    ")",
    "|",
    r"(?:",
    "简体中文|繁体中文|中文|英文|汉语|英语|简体|繁体",
    r")\s*(?:回答|回复|作答|回应|推理|思考|输出|表达|说明|解释)",
    "|",
    r"(?:用|使用|以)\s*(?:",
    "简体中文|繁体中文|中文|英文|汉语|英语|简体|繁体",
    r")\s*[。！!.]",
);

fn conflict_re() -> &'static Regex {
    static CELL: OnceLock<Regex> = OnceLock::new();
    CELL.get_or_init(|| Regex::new(&format!("(?i){}", CONFLICT_PATTERN)).expect("冲突正则应可编译"))
}

pub fn has_language_conflict(user_system_prompt: Option<&str>) -> bool {
    match user_system_prompt {
        Some(text) => conflict_re().is_match(text),
        None => false,
    }
}

#[derive(Debug, Clone)]
pub struct InjectionInput<'a> {
    pub model: &'a ModelMeta,
    pub profile: Option<&'a ModelProfile>,
    pub signals: &'a Signals,
    pub routing: &'a Routing,
    pub savings: Option<&'a SavingsResult>,
    pub user_system_prompt: Option<&'a str>,
    pub streaming: bool,
}

pub fn build_injection_plan(input: &InjectionInput<'_>) -> InjectionPlan {
    let target = input.routing.decided_lang;
    let disabled = |reason: String| InjectionPlan {
        state: InjectionState::Disabled,
        target_lang: target,
        system_prompt: String::new(),
        prompt_tokens: 0.0,
        tag_tokens: 0.0,
        reason,
    };

    if !input.model.reasoning_capable {
        return disabled("非推理模型没有独立推理 token，语言约束无收益".to_string());
    }

    if has_language_conflict(input.user_system_prompt) {
        return InjectionPlan {
            state: InjectionState::SkippedConflict,
            target_lang: target,
            system_prompt: String::new(),
            prompt_tokens: 0.0,
            tag_tokens: 0.0,
            reason: "检测到用户 system prompt 已规定语言，跳过注入".to_string(),
        };
    }

    if !input.routing.quality_first {
        let Some(savings) = input.savings else {
            return disabled("未校准或未通过成本闸门，不注入以免白付开销".to_string());
        };
        if !savings.switchable {
            return disabled(savings.reason.clone());
        }
    }

    let built = build_system_prompt(&PromptOptions {
        reasoning_lang: target,
        output_lang: input.routing.output_lang,
        streaming: input.streaming,
        use_tags: None,
    });

    let reason = if input.routing.quality_first {
        "质量优先：注入语言约束以稳定推理语言".to_string()
    } else {
        format!("净收益为正：{}", input.savings.map(|s| s.reason.as_str()).unwrap_or(""))
    };

    InjectionPlan {
        state: if input.user_system_prompt.is_some() {
            InjectionState::Appended
        } else {
            InjectionState::Injected
        },
        target_lang: target,
        system_prompt: built.text,
        prompt_tokens: built.prompt_tokens,
        tag_tokens: built.tag_tokens,
        reason,
    }
}
