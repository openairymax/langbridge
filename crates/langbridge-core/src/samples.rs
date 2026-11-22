//! 校准样本：中英含义对齐的通用技术语句（避开专有名词）。
//! N / 2N / 4N 三档共享同一段固定前导语，差分时固定开销被抵消。

use crate::types::Lang;

pub const PREAMBLE_EN: &str = "You are a careful assistant. Read the following passage and answer the question at the end. Keep the answer concise, and do not add information that is not supported by the passage.";
pub const PREAMBLE_ZH: &str = "你是一个严谨的助手。请阅读下面这段材料，并回答末尾的问题。请保持回答简洁，不要补充材料中没有依据的信息。";

pub const BODY_EN: [&str; 10] = [
    "A well designed system keeps its parts independent so that each one can be tested on its own.",
    "When a change is small and focused, it is easier to review, easier to roll back, and less likely to break something else.",
    "Teams often measure success by the number of features they ship, but stability and clarity matter just as much over time.",
    "Good documentation explains why a decision was made, not only what the code does today.",
    "Before optimizing a slow path, confirm with real data that the path is actually the bottleneck.",
    "Caching helps when reads are frequent and the data does not change often, yet it adds a second copy that can drift.",
    "A simple queue can smooth a burst of traffic, but it also moves failure from one place to another.",
    "Tests are most valuable when they describe behaviour that users care about and fail for a clear reason.",
    "Naming is a design activity: a precise name removes the need for a long comment.",
    "Every dependency you add is a promise to keep it updated, so prefer the smallest tool that solves the problem.",
];

pub const BODY_ZH: [&str; 10] = [
    "设计良好的系统会让各部分保持独立，这样每一部分都能单独测试。",
    "当改动范围小而且聚焦时，它更容易评审，更容易回滚，也更不容易破坏其他功能。",
    "团队常以发布功能的数量衡量成功，但长期来看，稳定性与清晰度同样重要。",
    "好的文档会解释当时为什么这样决定，而不只是说明代码今天在做什么。",
    "优化一条慢路径之前，先用真实数据确认它确实是瓶颈。",
    "当读取频繁且数据不常变化时，缓存会有帮助，但它也引入了可能不一致的第二份副本。",
    "一个简单的队列可以削平流量的尖峰，但它也会把故障从一个地方转移到另一个地方。",
    "当测试描述用户关心的行为、并在失败时给出清晰原因，它的价值最大。",
    "命名是一项设计活动：精确的名字可以省去一段很长的注释。",
    "每增加一个依赖，都是一份持续维护的承诺，因此应优先选择解决问题的最小工具。",
];

/// 三档语料规模
pub const SCALES: [u32; 3] = [1, 2, 4];

pub fn body_text(lang: Lang, repeat: u32) -> String {
    let src: &[&str] = match lang {
        Lang::Zh => &BODY_ZH,
        Lang::En => &BODY_EN,
    };
    let sep = match lang {
        Lang::Zh => "",
        Lang::En => " ",
    };
    let mut parts: Vec<&str> = Vec::new();
    for _ in 0..repeat {
        parts.extend_from_slice(src);
    }
    parts.join(sep)
}

pub fn build_sample(lang: Lang, repeat: u32) -> String {
    let (preamble, sep) = match lang {
        Lang::Zh => (PREAMBLE_ZH, ""),
        Lang::En => (PREAMBLE_EN, " "),
    };
    format!("{}{}{}", preamble, sep, body_text(lang, repeat))
}

/// 中文按每份 120 字计；英文按实际词数
pub fn word_count(lang: Lang, repeat: u32) -> usize {
    match lang {
        Lang::Zh => 120 * repeat as usize,
        Lang::En => body_text(Lang::En, repeat)
            .split_whitespace()
            .filter(|w| !w.is_empty())
            .count(),
    }
}
