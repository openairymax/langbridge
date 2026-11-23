//! 内置数据：空态参考表、首屏示例与校准样本。

use langbridge_core::evidence::{calibrate_from_evidence, list_evidence_models};
use langbridge_core::profiles::get_model;

pub use langbridge_core::samples::{
    body_text, build_sample, word_count, BODY_EN, BODY_ZH, PREAMBLE_EN, PREAMBLE_ZH, SCALES,
};

#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinRow {
    pub id: String,
    pub name: String,
    pub ratio: f64,
}

pub fn builtin_rows() -> Vec<BuiltinRow> {
    list_evidence_models()
        .into_iter()
        .filter_map(|id| {
            calibrate_from_evidence(&id).ok().map(|cal| BuiltinRow {
                name: get_model(&id)
                    .map(|m| m.display_name)
                    .unwrap_or_else(|| id.clone()),
                ratio: cal.effective_ratio,
                id,
            })
        })
        .collect()
}

pub fn demo_model_id() -> Option<String> {
    let rows = builtin_rows();
    let pick = |only_reasoning: bool| {
        rows.iter()
            .filter(|row| {
                !only_reasoning
                    || get_model(&row.id)
                        .map(|m| m.reasoning_capable)
                        .unwrap_or(false)
            })
            .max_by(|a, b| gap(a.ratio).total_cmp(&gap(b.ratio)))
            .map(|row| row.id.clone())
    };
    pick(true).or_else(|| pick(false))
}

fn gap(ratio: f64) -> f64 {
    if ratio >= 1.0 { ratio } else { 1.0 / ratio }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub id: &'static str,
    pub label: &'static str,
    pub hint: &'static str,
    pub text: &'static str,
}

pub const EXAMPLES: [Example; 4] = [
    Example {
        id: "code",
        label: "代码任务",
        hint: "预期：以 DeepSeek 为例，代码任务建议中文推理",
        text: "用 Python 写一个快速排序，并解释时间复杂度。代码要能直接运行，最后给出三个测试用例。",
    },
    Example {
        id: "culture",
        label: "文化典故",
        hint: "预期：文化内容固定用中文，且不给成本结论",
        text: "解释成语「塞翁失马」的出处和寓意，并仿写一段文言小故事。",
    },
    Example {
        id: "long",
        label: "长文推理",
        hint: "预期：中文更省、不用换语言，省不过门槛就直说",
        text: "我们要为一个跨境电商平台设计订单履约系统。订单可能来自多个渠道，库存分布在三个仓库，还需要处理退款和换货。请给出整体架构、关键数据模型，以及高峰期的限流与降级策略，并说明每个决策的取舍。",
    },
    Example {
        id: "english",
        label: "英文输入",
        hint: "预期：输出跟随英文，省过门槛才换语言，不过就直说",
        text: "Explain how a tokenizer splits Chinese and English text differently, and why that difference changes inference cost. Give one concrete example with numbers.",
    },
];

pub fn example_by_id(id: &str) -> Option<Example> {
    EXAMPLES.iter().find(|e| e.id == id).cloned()
}
