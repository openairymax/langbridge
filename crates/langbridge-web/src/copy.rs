//! 界面文案：组件的可见文本集中于此。

use langbridge_core::calibrate::LINEAR_MAX_DEVIATION;
use langbridge_core::evidence::{evidence_meta, has_evidence, list_evidence_models, ProbeStep};
use langbridge_core::guard::DRIFT_THRESHOLD;
use langbridge_core::profile_store::PROFILE_MAX_AGE_DAYS;
use langbridge_core::samples::SCALES;
use langbridge_core::savings::{SWITCH_THRESHOLD_PCT_DEFAULT, SWITCH_THRESHOLD_TOKENS_DEFAULT};
use langbridge_core::types::{
    CalibrationResult, CostStatus, DecidedBy, InjectionPlan, InjectionState, Lang, ProfileSource,
    Routing, RoutingStep, SavingsResult, Signals, TaskType,
};

use crate::state::{Evaluation, ProfileHealth};

pub fn lang_text(value: Lang) -> &'static str {
    match value {
        Lang::Zh => "中文",
        Lang::En => "英文",
    }
}

pub fn method_label_source(method: ProfileSource) -> &'static str {
    match method {
        ProfileSource::Builtin => "内置参考数据",
        ProfileSource::ApiProbe => "API 测量",
        ProfileSource::LiveRun => "真实使用回流",
        ProfileSource::Manual => "手动录入",
    }
}

pub fn method_label(method: &str) -> &str {
    match method {
        "builtin" => "内置参考数据",
        "api_probe" => "API 测量",
        "live_run" => "真实使用回流",
        "manual" => "手动录入",
        other => other,
    }
}

impl EmptyCopy {
    pub fn builtin_count(&self, count: usize) -> String {
        empty_builtin_count(count)
    }
    pub fn builtin_hint(&self) -> String {
        empty_builtin_hint()
    }
}

impl DemoCopy {
    pub fn hint(&self, row_text: &str) -> String {
        demo_hint(row_text)
    }
}

pub const LAYER_ORDER: [langbridge_core::types::RoutingLayer; 4] = [
    langbridge_core::types::RoutingLayer::L1,
    langbridge_core::types::RoutingLayer::L2,
    langbridge_core::types::RoutingLayer::L3,
    langbridge_core::types::RoutingLayer::L4,
];

pub fn layers() -> [&'static LayerCopy; 4] {
    [&LAYER_L1, &LAYER_L2, &LAYER_L3, &LAYER_L4]
}

fn scales_txt() -> String {
    SCALES
        .iter()
        .map(|s| format!("{}N", s))
        .collect::<Vec<_>>()
        .join(" / ")
}

fn js_num(v: f64) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v.fract() == 0.0 {
        format!("{:.0}", v)
    } else {
        format!("{}", v)
    }
}

fn round_i(v: f64) -> String {
    format!("{:.0}", v.round())
}

pub fn fmt2(v: f64) -> String {
    format!("{:.2}", v)
}

pub fn fmt3(v: f64) -> String {
    format!("{:.3}", v)
}

pub fn thousands(n: i64) -> String {
    let negative = n < 0;
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    if negative {
        format!("-{}", out)
    } else {
        out
    }
}

pub struct OriginCopy {
    pub org: &'static str,
    pub url: &'static str,
    pub text: &'static str,
}

pub struct HeroCopy {
    pub app_name: &'static str,
    pub title: &'static str,
    pub sub: &'static str,
    pub origin: OriginCopy,
}

pub const HERO: HeroCopy = HeroCopy {
    app_name: "LangBridge",
    title: "模型使用中文和英文推理成本不同，哪个推理特征更省、能省多少 Token",
    sub: "先测这个模型的中英成本比，再决定要不要换语言。省不过门槛，就明说不建议换。",
    origin: OriginCopy {
        org: "OpenAirymax",
        url: "https://atomgit.com/openairymax",
        text: " 开源项目的系统性省 Token 工程部分",
    },
};

pub const OFFLINE_BADGE: &str = "全程在本机完成";

pub const STEPS_TITLE: &str = "它怎么判断";

pub struct StepCopy {
    pub no: &'static str,
    pub title: &'static str,
    pub text: String,
}

pub fn steps() -> Vec<StepCopy> {
    vec![
        StepCopy {
            no: "01",
            title: "读内容",
            text: "本地判断提问语言、任务类型、有没有代码。".to_string(),
        },
        StepCopy {
            no: "02",
            title: "查换算数据",
            text: format!("中英各测 {}，相邻两档相减，抵掉固定开销。", scales_txt()),
        },
        StepCopy {
            no: "03",
            title: "先保质量，再算成本",
            text: "文化、政策固定中文；省得过门槛才建议切。".to_string(),
        },
    ]
}

pub const STEPS_NOTE: &str = "规则取自生产网关的四层实践，这里是本地版。";

pub struct EmptyCopy {
    pub kicker: &'static str,
    pub hint: &'static str,
    pub builtin_title: &'static str,
    pub items: [&'static str; 3],
}

pub const EMPTY: EmptyCopy = EmptyCopy {
    kicker: "结论会出现在这里",
    hint: "粘贴问题后，这里给出：用哪种语言思考、要不要切、省多少 Token。",
    builtin_title: "内置参考数据",
    items: [
        "文化、政策：固定中文",
        "省得过门槛：才建议切",
        "没有数据：不谈成本",
    ],
};

pub fn empty_builtin_count(count: usize) -> String {
    format!("{} 个模型", count)
}

pub fn empty_builtin_hint() -> String {
    format!("中英同内容各测 {}，相减抵掉固定开销。", scales_txt())
}

pub struct DemoCopy {
    pub button: &'static str,
    pub hint: &'static str,
}

pub const DEMO: DemoCopy = DemoCopy {
    button: "用内置数据看一次",
    hint: "自动选一个内置模型，填入示例，直接给出结论",
};

pub fn demo_hint(row_text: &str) -> String {
    format!("载入示例，直接看结论：{}", row_text)
}

pub fn builtin_row_text(ratio: f64) -> String {
    let zh_bigger = ratio >= 1.0;
    let multiple = if zh_bigger { ratio } else { 1.0 / ratio };
    if zh_bigger {
        format!("中文 Token 是英文的 {} 倍", fmt2(multiple))
    } else {
        format!("英文 Token 是中文的 {} 倍", fmt2(multiple))
    }
}

pub struct LayerCopy {
    pub name: &'static str,
    pub rule: &'static str,
}

pub const LAYER_L1: LayerCopy = LayerCopy {
    name: "内容规则",
    rule: "文化、政策：固定中文",
};
pub const LAYER_L2: LayerCopy = LayerCopy {
    name: "模型特点",
    rule: "部分模型代码、数学用中文更稳",
};
pub const LAYER_L3: LayerCopy = LayerCopy {
    name: "输入语言",
    rule: "思考语言跟随提问语言",
};
pub const LAYER_L4: LayerCopy = LayerCopy {
    name: "成本计算",
    rule: "真更省才切",
};

pub fn layer(layer: langbridge_core::types::RoutingLayer) -> &'static LayerCopy {
    use langbridge_core::types::RoutingLayer::*;
    match layer {
        L1 => &LAYER_L1,
        L2 => &LAYER_L2,
        L3 => &LAYER_L3,
        L4 => &LAYER_L4,
    }
}

pub fn why_mark(step: Option<&RoutingStep>) -> &'static str {
    match step {
        Some(s) if s.hit => "起作用了",
        Some(s) if s.terminated => "到此为止",
        _ => "没涉及",
    }
}

fn decided_by_label(decided_by: DecidedBy) -> &'static str {
    match decided_by {
        DecidedBy::L1 => "内容规则",
        DecidedBy::L2 => "模型特点",
        DecidedBy::L3 => "输入语言",
        DecidedBy::L4 => "成本计算",
        DecidedBy::L4Blocked => "成本计算",
        DecidedBy::Fallback => "综合判断",
    }
}

pub fn chain_foot(routing: &Routing) -> String {
    format!(
        "{}决定 · 回答用{}",
        decided_by_label(routing.decided_by),
        lang_text(routing.output_lang)
    )
}

pub struct ChainCopy {
    pub title: &'static str,
    pub unevaluated: &'static str,
}

pub const CHAIN: ChainCopy = ChainCopy {
    title: "判断过程",
    unevaluated: "未评估",
};

pub struct SectionCopy {
    pub calibration: &'static str,
    pub decision: &'static str,
    pub evidence: &'static str,
}

pub const SECTION: SectionCopy = SectionCopy {
    calibration: "参考数据与测量",
    decision: "判断与费用",
    evidence: "数据来源与进阶",
};

pub struct SuffixCopy {
    pub label: &'static str,
    pub hint: &'static str,
    pub copy: &'static str,
}

pub const SUFFIX: SuffixCopy = SuffixCopy {
    label: "加到提示词末尾",
    hint: "告诉模型用什么语言思考，需要时才更省。",
    copy: "复制这句",
};

pub fn prompt_suffix(routing: &Routing) -> String {
    let think = lang_text(routing.decided_lang);
    let answer = lang_text(routing.output_lang);
    if think == answer {
        format!("请用{}思考，用{}回答。", think, think)
    } else {
        format!("请用{}思考，用{}回答。", think, answer)
    }
}

pub const FOLD_EMPTY_DECISION: &str = "输入问题后，这里展开判断过程与费用对照。";

pub struct FooterCopy {
    pub line: &'static str,
    pub note: &'static str,
    pub org: &'static str,
    pub org_url: &'static str,
}

pub const FOOTER: FooterCopy = FooterCopy {
    line: "全部在本机完成：问题不上传，不产生费用。只有你主动跑 API 测量，才直连你填的服务商。",
    note: "只反映 Token 数量差，不等于账单；单价不同，结论会变。",
    org: "OpenAirymax 开源极境",
    org_url: "https://atomgit.com/openairymax",
};

pub struct UiCopy {
    pub ask_aria: &'static str,
    pub input_aria: &'static str,
    pub try_label: &'static str,
    pub try_group_aria: &'static str,
    pub clear_input: &'static str,
    pub model_label: &'static str,
    pub model_select_aria: &'static str,
    pub model_custom_aria: &'static str,
    pub model_custom_placeholder: &'static str,
    pub other_model: &'static str,
    pub measured_group: &'static str,
    pub no_reasoning: &'static str,
    pub why_summary: &'static str,
    pub running: &'static str,
    pub measure: &'static str,
    pub load_ref: &'static str,
    pub reload_ref: &'static str,
    pub remeasure: &'static str,
    pub start_measure: &'static str,
    pub rerun: &'static str,
    pub run_api: &'static str,
    pub measuring: &'static str,
    pub retry: &'static str,
    pub clear_notice: &'static str,
    pub copy_json: &'static str,
    pub copied: &'static str,
    pub copy_failed: &'static str,
    pub download_json: &'static str,
    pub import_json: &'static str,
    pub import_record: &'static str,
    pub delete_record: &'static str,
    pub confirm_delete: &'static str,
    pub cancel: &'static str,
}

pub const UI: UiCopy = UiCopy {
    ask_aria: "输入你的问题",
    input_aria: "输入文本",
    try_label: "试试",
    try_group_aria: "试试这些例子",
    clear_input: "清空",
    model_label: "我在用",
    model_select_aria: "选择模型",
    model_custom_aria: "模型 ID",
    model_custom_placeholder: "输入模型 ID，如 kimi-k2-0905-preview",
    other_model: "其他模型…",
    measured_group: "我的测量",
    no_reasoning: "没有独立思考过程",
    why_summary: "为什么这么建议？",
    running: "处理中…",
    measure: "测一下",
    load_ref: "载入参考数据",
    reload_ref: "重新载入参考数据",
    remeasure: "重新测一次",
    start_measure: "开始测量",
    rerun: "再测一次（验证一致性）",
    run_api: "运行 API 测量",
    measuring: "测量中…",
    retry: "重试",
    clear_notice: "清除提示",
    copy_json: "复制 JSON",
    copied: "已复制",
    copy_failed: "复制失败",
    download_json: "下载 profile.json",
    import_json: "导入 profile.json",
    import_record: "导入记录",
    delete_record: "删除记录",
    confirm_delete: "确认删除",
    cancel: "取消",
};

impl UiCopy {
    pub fn input_placeholder(&self, first_example: &str) -> String {
        let short: String = first_example.chars().take(24).collect();
        format!("把你的问题粘贴在这里，例如：{}…", short)
    }

    pub fn char_count(&self, n: usize) -> String {
        format!("{} 字符", thousands(n as i64))
    }
}

pub fn action_label(opts: &ActionLabelOpts) -> String {
    if opts.running {
        return UI.running.to_string();
    }
    if opts.kind == ModelNoteKind::None {
        return if opts.has_evidence {
            UI.load_ref.to_string()
        } else {
            UI.measure.to_string()
        };
    }
    if opts.method.is_none() || opts.method == Some("builtin") {
        UI.reload_ref.to_string()
    } else {
        UI.remeasure.to_string()
    }
}

pub struct ActionLabelOpts<'a> {
    pub running: bool,
    pub kind: ModelNoteKind,
    pub method: Option<&'a str>,
    pub has_evidence: bool,
}

pub fn task_label(task: TaskType) -> &'static str {
    match task {
        TaskType::Code => "代码",
        TaskType::Math => "数学",
        TaskType::Culture => "文化",
        TaskType::Policy => "政策",
        TaskType::General => "通用",
    }
}

pub struct SignalRow {
    pub label: &'static str,
    pub value: String,
    pub sub: Option<String>,
}

pub fn signal_rows(signals: &Signals, chars: usize) -> Vec<SignalRow> {
    let mut rows = vec![
        SignalRow {
            label: "提问语言",
            value: lang_text(signals.lang).to_string(),
            sub: Some(fmt2(signals.lang_confidence)),
        },
        SignalRow {
            label: "内容类型",
            value: task_label(signals.task).to_string(),
            sub: None,
        },
        SignalRow {
            label: "包含代码",
            value: if signals.has_code { "是" } else { "否" }.to_string(),
            sub: None,
        },
        SignalRow {
            label: "长度",
            value: format!("{} 字符 · 约 {} Token", chars, signals.input_tokens_est),
            sub: Some("（估算）".to_string()),
        },
    ];
    if signals.mixed {
        rows.push(SignalRow {
            label: "中英混合",
            value: format!("中文 {:.0} / 英文 {:.1}", signals.zh_mass, signals.en_mass),
            sub: None,
        });
    }
    rows
}

fn injection_state_key(state: InjectionState) -> &'static str {
    match state {
        InjectionState::Injected => "injected",
        InjectionState::Appended => "appended",
        InjectionState::SkippedConflict => "skipped_conflict",
        InjectionState::Disabled => "disabled",
    }
}

pub fn inject_text(injection: Option<&InjectionPlan>) -> String {
    match injection {
        Some(plan) => format!("{} · {}", injection_state_key(plan.state), plan.reason),
        None => "无".to_string(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelNoteKind {
    Builtin,
    Measured,
    Stale,
    LowQuality,
    None,
}

pub struct ModelNote {
    pub kind: ModelNoteKind,
    pub lead: String,
    pub multiple: Option<f64>,
    pub tail: String,
}

fn tokenizer_source(measured: &CalibrationResult) -> String {
    let tk = &measured.tokenizer;
    if tk.exact == Some(false) {
        format!("同系列公开 tokenizer {}", tk.name)
    } else {
        format!("公开 tokenizer {}", tk.name)
    }
}

fn days_or(value: Option<f64>, fallback: &str) -> String {
    match value {
        Some(d) if d.is_finite() => format!("{:.0}", d),
        _ => fallback.to_string(),
    }
}

pub fn model_note(evaluation: &Evaluation) -> ModelNote {
    let status = &evaluation.profile_status;
    let Some(profile) = evaluation.profile.as_ref() else {
        return ModelNote {
            kind: ModelNoteKind::None,
            lead: String::new(),
            multiple: None,
            tail: if has_evidence(&evaluation.model.model_id) {
                format!(
                    "还没载入 {} 的参考数据，载入后估算节省。",
                    evaluation.model.display_name
                )
            } else {
                format!(
                    "还没测过 {}，用 API 测一次后算节省。",
                    evaluation.model.display_name
                )
            },
        };
    };
    let measured = &profile.measurement;
    let ratio = measured.effective_ratio;
    let bigger = if ratio >= 1.0 { Lang::Zh } else { Lang::En };
    let other = if bigger == Lang::Zh {
        Lang::En
    } else {
        Lang::Zh
    };
    let multiple = if ratio >= 1.0 { ratio } else { 1.0 / ratio };
    let days = status.age_days.map(|d| d as f64);
    let is_builtin = measured.method == ProfileSource::Builtin;
    let source = if is_builtin {
        tokenizer_source(measured)
    } else {
        method_label_source(measured.method).to_string()
    };

    if is_builtin {
        if status.health == ProfileHealth::Stale {
            return ModelNote {
                kind: ModelNoteKind::Stale,
                lead: format!(
                    "内置参考数据（{} 天前，{} 真实计数）：{} Token 约是{}的",
                    days_or(days, "若干"),
                    source,
                    lang_text(bigger),
                    lang_text(other)
                ),
                multiple: Some(multiple),
                tail: format!(
                    "倍。已超 {} 天有效期，精确计费请用 API 测量。",
                    PROFILE_MAX_AGE_DAYS
                ),
            };
        }
        if status.health == ProfileHealth::LowQuality {
            return ModelNote {
                kind: ModelNoteKind::LowQuality,
                lead: format!(
                    "内置参考数据（{} 真实计数）置信度 {} 未达标，",
                    source,
                    fmt2(measured.confidence)
                ),
                multiple: None,
                tail: "暂不参与成本估算，精确计费请用 API 测量。".to_string(),
            };
        }
        return ModelNote {
            kind: ModelNoteKind::Builtin,
            lead: format!(
                "内置参考（{} 真实计数）：{} Token 约是{}的",
                source,
                lang_text(bigger),
                lang_text(other)
            ),
            multiple: Some(multiple),
            tail: "倍。服务端计费可能略有差异，可用 API 测量复核。".to_string(),
        };
    }

    if status.health == ProfileHealth::Stale {
        return ModelNote {
            kind: ModelNoteKind::Stale,
            lead: format!(
                "上次测量（{}，{} 天前）：{} Token 是{}的",
                source,
                days_or(days, "若干"),
                lang_text(bigger),
                lang_text(other)
            ),
            multiple: Some(multiple),
            tail: format!("倍。已超 {} 天有效期，重测后恢复。", PROFILE_MAX_AGE_DAYS),
        };
    }

    if status.health == ProfileHealth::LowQuality {
        return ModelNote {
            kind: ModelNoteKind::LowQuality,
            lead: format!(
                "测量于 {} 天前（{}），置信度 {} 未达标，",
                days_or(status.age_days.map(|d| d as f64), "0"),
                source,
                fmt2(measured.confidence)
            ),
            multiple: None,
            tail: "暂不出成本结论，重测后恢复。".to_string(),
        };
    }

    ModelNote {
        kind: ModelNoteKind::Measured,
        lead: format!(
            "已测量（{}，{} 天前）：{} Token 是{}的",
            source,
            days_or(status.age_days.map(|d| d as f64), "0"),
            lang_text(bigger),
            lang_text(other)
        ),
        multiple: Some(multiple),
        tail: "倍。".to_string(),
    }
}

pub struct RatioBarRow {
    pub key: Lang,
    pub label: &'static str,
    pub value: f64,
}

pub struct RatioBarsView {
    pub head: &'static str,
    pub sample: String,
    pub rows: Vec<RatioBarRow>,
    pub foot: String,
    pub aria: String,
}

pub fn ratio_bars_view(cal: &CalibrationResult) -> Option<RatioBarsView> {
    if cal.observations.is_empty() {
        return None;
    }
    let n = cal
        .observations
        .iter()
        .map(|o| o.n)
        .fold(f64::INFINITY, f64::min);
    let at = |l: Lang| {
        cal.observations
            .iter()
            .find(|o| o.lang == l && o.n == n)
            .map(|o| o.prompt_tokens)
            .unwrap_or(0.0)
    };
    let zh = at(Lang::Zh);
    let en = at(Lang::En);
    if zh == 0.0 || en == 0.0 {
        return None;
    }
    let method = if cal.method == ProfileSource::Builtin {
        "参考"
    } else {
        "测量"
    };
    let winner = if zh == en {
        "两者消耗相同".to_string()
    } else if zh < en {
        "中文更省 Token".to_string()
    } else {
        "英文更省 Token".to_string()
    };
    Some(RatioBarsView {
        head: "同样的内容",
        sample: format!("n={} 的{}样本", js_num(n), method),
        rows: vec![
            RatioBarRow {
                key: Lang::Zh,
                label: "中文",
                value: zh,
            },
            RatioBarRow {
                key: Lang::En,
                label: "英文",
                value: en,
            },
        ],
        foot: format!(
            "行内为 n={} 的原始计数，比值为差分斜率比 {}。",
            js_num(n),
            fmt2(cal.effective_ratio)
        ),
        aria: format!(
            "中英 Token 对照：同样内容中文 {} Token、英文 {} Token；{}（差分斜率比 {}）。",
            js_num(zh),
            js_num(en),
            winner,
            fmt2(cal.effective_ratio)
        ),
    })
}

pub struct Verdict {
    pub kicker: String,
    pub headline: String,
    pub detail: String,
}

pub fn verdict_copy(evaluation: &Evaluation) -> Verdict {
    let routing = &evaluation.routing;
    let savings = evaluation.savings.as_ref();
    let model = &evaluation.model;
    let status = &evaluation.profile_status;
    let kicker = format!("建议 · {}", model.display_name);
    let output = lang_text(routing.output_lang);
    let decided = lang_text(routing.decided_lang);

    if routing.decided_by == DecidedBy::L1 {
        return Verdict {
            kicker,
            headline: "用中文思考".to_string(),
            detail: "这类内容靠中文语境，先保准确，不为省 Token 换语言。".to_string(),
        };
    }

    if routing.decided_by == DecidedBy::L2 {
        return Verdict {
            kicker,
            headline: "用中文思考".to_string(),
            detail: format!(
                "{} 做代码、数学用中文更稳，不拿质量换 Token。",
                model.display_name
            ),
        };
    }

    if routing.decided_by == DecidedBy::L3 {
        return Verdict {
            kicker,
            headline: format!("和提问一致：用{}思考", decided),
            detail: format!(
                "提问是{}，思考也一样，不用额外说明，也不多花 Token。",
                lang_text(evaluation.signals.lang)
            ),
        };
    }

    if routing.decided_by == DecidedBy::L4Blocked {
        let uses_builtin = evaluation
            .profile
            .as_ref()
            .map(|p| p.measurement.method == ProfileSource::Builtin)
            .unwrap_or(false);
        let can_estimate = has_evidence(&evaluation.model.model_id);
        let headline = if status.health == ProfileHealth::Stale {
            if uses_builtin {
                "参考数据过期了，重新载入才能估算"
            } else {
                "测量过期了，重测才能算"
            }
        } else if status.health == ProfileHealth::LowQuality {
            if uses_builtin {
                "参考数据质量不够，暂时算不出"
            } else {
                "测量质量不够，暂时算不出"
            }
        } else if can_estimate {
            "先载入参考数据，才能估算"
        } else {
            "先测一下，才能算省多少"
        };
        let detail = if status.health == ProfileHealth::Stale {
            if uses_builtin {
                format!(
                    "参考数据在 {} 天前，超过 {} 天有效期，成本结论已停用。",
                    days_or(status.age_days.map(|d| d as f64), "若干"),
                    PROFILE_MAX_AGE_DAYS
                )
            } else {
                format!(
                    "上次测量在 {} 天前，超过 {} 天有效期，成本结论已停用。",
                    days_or(status.age_days.map(|d| d as f64), "若干"),
                    PROFILE_MAX_AGE_DAYS
                )
            }
        } else if status.health == ProfileHealth::LowQuality {
            if uses_builtin {
                format!(
                    "参考数据置信度 {}，达不到出结论标准。",
                    evaluation
                        .profile
                        .as_ref()
                        .map(|p| fmt2(p.measurement.confidence))
                        .unwrap_or_else(|| "偏低".to_string())
                )
            } else {
                format!(
                    "这次测量置信度 {}，达不到出结论标准。",
                    evaluation
                        .profile
                        .as_ref()
                        .map(|p| fmt2(p.measurement.confidence))
                        .unwrap_or_else(|| "偏低".to_string())
                )
            }
        } else if can_estimate {
            "载入后补上节省估算；要精确值用 API 测量。".to_string()
        } else {
            "先按质量给建议；测一次后补上节省数字。".to_string()
        };
        return Verdict {
            kicker,
            headline: headline.to_string(),
            detail,
        };
    }

    let basis = if evaluation
        .profile
        .as_ref()
        .map(|p| p.measurement.method == ProfileSource::Builtin)
        .unwrap_or(false)
    {
        "按内置参考数据估算："
    } else {
        ""
    };

    if let Some(s) = savings {
        if s.switchable {
            return Verdict {
                kicker,
                headline: format!("换成{}思考更省 Token", decided),
                detail: format!(
                    "{}同样的内容，{}比{}少用约 {}% 思考 Token，扣掉说明仍划算。回答用{}。",
                    basis,
                    decided,
                    lang_text(s.current),
                    round_i((1.0 - s.multiplier) * 100.0),
                    output
                ),
            };
        }
        if s.current != s.target && s.net > 0.0 {
            return Verdict {
                kicker,
                headline: format!("不用换语言，继续用{}思考", output),
                detail: format!(
                    "{}换语言只净省 {} Token，不到 {} 的门槛，不值得换。回答继续用{}。",
                    basis,
                    js_num(s.net),
                    js_num(s.threshold),
                    output
                ),
            };
        }
        if s.current != s.target {
            return Verdict {
                kicker,
                headline: "不用换语言，换反而更贵".to_string(),
                detail: format!(
                    "{}换语言会多花 {} Token，不划算。回答继续用{}。",
                    basis,
                    js_num(s.net.abs()),
                    output
                ),
            };
        }
    }

    Verdict {
        kicker,
        headline: format!("保持{}思考", decided),
        detail: format!(
            "{}切换不划算：{}。回答用{}。",
            basis,
            savings
                .map(|s| s.reason.as_str())
                .unwrap_or("没有明显的 Token 优势"),
            output
        ),
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum SavingsStory {
    Save {
        net: f64,
        pct: f64,
        overhead: f64,
        r: f64,
        text: String,
    },
    Lose {
        loss: f64,
        text: String,
    },
    Below {
        net: f64,
        threshold: f64,
        text: String,
    },
    Same {
        text: String,
    },
    Blocked {
        text: String,
    },
    Quality {
        text: String,
    },
    Na {
        text: String,
    },
}

impl SavingsStory {
    pub fn kind(&self) -> &'static str {
        match self {
            SavingsStory::Save { .. } => "save",
            SavingsStory::Lose { .. } => "lose",
            SavingsStory::Below { .. } => "below",
            SavingsStory::Same { .. } => "same",
            SavingsStory::Blocked { .. } => "blocked",
            SavingsStory::Quality { .. } => "quality",
            SavingsStory::Na { .. } => "na",
        }
    }

    pub fn text(&self) -> &str {
        match self {
            SavingsStory::Save { text, .. }
            | SavingsStory::Lose { text, .. }
            | SavingsStory::Below { text, .. }
            | SavingsStory::Same { text }
            | SavingsStory::Blocked { text }
            | SavingsStory::Quality { text }
            | SavingsStory::Na { text } => text,
        }
    }

    pub fn net(&self) -> Option<f64> {
        match self {
            SavingsStory::Save { net, .. } | SavingsStory::Below { net, .. } => Some(*net),
            _ => None,
        }
    }
}

pub fn savings_story(evaluation: &Evaluation) -> SavingsStory {
    let routing = &evaluation.routing;
    let savings = evaluation.savings.as_ref();
    let status = &evaluation.profile_status;
    let basis = if evaluation
        .profile
        .as_ref()
        .map(|p| p.measurement.method == ProfileSource::Builtin)
        .unwrap_or(false)
    {
        "按内置参考数据估算："
    } else {
        ""
    };

    if !evaluation.model.reasoning_capable {
        return SavingsStory::Na {
            text: "这个模型没有独立思考，没有可省的推理 Token。".to_string(),
        };
    }

    if routing.decided_by == DecidedBy::L1 || routing.decided_by == DecidedBy::L2 {
        return SavingsStory::Quality {
            text: "质量优先：不拿准确度换 Token，不计节省。".to_string(),
        };
    }

    let Some(s) = savings else {
        if routing.decided_by == DecidedBy::L4Blocked {
            let uses_builtin = evaluation
                .profile
                .as_ref()
                .map(|p| p.measurement.method == ProfileSource::Builtin)
                .unwrap_or(false);
            let can_estimate = has_evidence(&evaluation.model.model_id);
            let reason = if status.health == ProfileHealth::Stale {
                if uses_builtin {
                    "参考数据过期，重新载入才有节省数字。"
                } else {
                    "测量过期，重测才有节省数字。"
                }
            } else if status.health == ProfileHealth::LowQuality {
                if uses_builtin {
                    "参考数据置信度不够，暂不出节省数字。"
                } else {
                    "测量置信度不够，重测才有节省数字。"
                }
            } else if can_estimate {
                "还没载入参考数据，载入后显示节省。"
            } else {
                "还没测过，测一次后显示节省。"
            };
            return SavingsStory::Blocked {
                text: reason.to_string(),
            };
        }
        return SavingsStory::Same {
            text: "思考语言本来就合适，不用切，也不多花 Token。".to_string(),
        };
    };

    if s.current == s.target || (s.net == 0.0 && s.overhead == 0.0) {
        return SavingsStory::Same {
            text: s.reason.clone(),
        };
    }

    if s.net <= 0.0 {
        return SavingsStory::Lose {
            loss: s.net.abs(),
            text: format!(
                "{}切换反而多花 {} Token，保持现状。",
                basis,
                js_num(s.net.abs())
            ),
        };
    }

    if !s.switchable {
        return SavingsStory::Below {
            net: s.net,
            threshold: s.threshold,
            text: format!(
                "{}切换只净省 {} Token，低于 {} 的门槛，不值得切。",
                basis,
                js_num(s.net),
                js_num(s.threshold)
            ),
        };
    }

    let pct = (s.net / s.r * 100.0).round();
    SavingsStory::Save {
        net: s.net,
        pct,
        overhead: s.overhead,
        r: s.r,
        text: format!(
            "{}扣掉换语言的 {} Token，每次净省 {} Token（{}%）。",
            basis,
            js_num(s.overhead),
            js_num(s.net),
            js_num(pct)
        ),
    }
}

pub struct SaveLineParts {
    pub lead: String,
    pub tail: String,
}

pub fn save_line_parts(story: &SavingsStory) -> Option<SaveLineParts> {
    match story {
        SavingsStory::Save {
            r, overhead, pct, ..
        } => Some(SaveLineParts {
            lead: format!(
                "按 {} Token 思考量，扣掉 {} Token 换语言说明，每次净省 ",
                js_num(*r),
                js_num(*overhead)
            ),
            tail: format!(" Token（{}%）。", js_num(*pct)),
        }),
        _ => None,
    }
}

pub struct ResultCopy {
    pub copy: &'static str,
    pub download: &'static str,
    pub download_started: &'static str,
    pub download_failed: &'static str,
    pub copied: &'static str,
    pub copy_failed: &'static str,
    pub brand: &'static str,
    pub card_title: &'static str,
    pub card_foot: &'static str,
    pub model_label: &'static str,
    pub ratio_label: &'static str,
    pub net_label: &'static str,
    pub net_potential: &'static str,
    pub net_blocked: &'static str,
    pub net_same: &'static str,
    pub net_loss: &'static str,
    pub suffix_label: &'static str,
    pub time_label: &'static str,
    pub unloaded: &'static str,
    pub not_applicable: &'static str,
}

pub const RESULT: ResultCopy = ResultCopy {
    copy: "复制整段结论",
    download: "下载结果卡",
    download_started: "已开始下载",
    download_failed: "下载失败，请改用复制",
    copied: "已复制",
    copy_failed: "复制失败，请手动选择",
    brand: "LangBridge",
    card_title: "LangBridge 结论卡",
    card_foot: "本卡由 LangBridge 在本机生成；数字来自内置参考数据的 Token 估算，不代表实际账单。",
    model_label: "模型",
    ratio_label: "中英 Token 比",
    net_label: "本次净省",
    net_potential: "切换可省",
    net_blocked: "未过门槛",
    net_same: "同语言，无需切换",
    net_loss: "切换反而多花",
    suffix_label: "建议追加",
    time_label: "生成时间",
    unloaded: "未载入",
    not_applicable: "不适用",
};

pub fn result_ratio_text(ratio: Option<f64>) -> String {
    match ratio {
        None => format!("{} {}", RESULT.ratio_label, RESULT.unloaded),
        Some(r) => format!("{} {}×", RESULT.ratio_label, fmt2(r)),
    }
}

pub fn result_net_text(savings: Option<&SavingsResult>) -> String {
    let Some(s) = savings else {
        return format!("{} {}", RESULT.net_label, RESULT.not_applicable);
    };
    if s.current == s.target {
        return RESULT.net_same.to_string();
    }
    if s.switchable {
        return format!("{} {} Token", RESULT.net_label, js_num(s.net));
    }
    if s.net > 0.0 {
        return format!(
            "{} {} · {} {}",
            RESULT.net_potential,
            js_num(s.net),
            RESULT.net_blocked,
            js_num(s.threshold)
        );
    }
    format!("{} {} Token", RESULT.net_loss, js_num(s.net.abs()))
}

fn follows_input(evaluation: &Evaluation) -> bool {
    evaluation.savings.is_none()
        && evaluation.routing.decided_by == DecidedBy::L3
        && evaluation.routing.cost_status == CostStatus::Ok
}

pub fn result_net_text_for(evaluation: &Evaluation) -> String {
    if follows_input(evaluation) {
        RESULT.net_same.to_string()
    } else {
        result_net_text(evaluation.savings.as_ref())
    }
}

pub fn result_summary_text(evaluation: &Evaluation, generated_at: &str) -> String {
    let verdict = verdict_copy(evaluation);
    let story = savings_story(evaluation);
    let mut lines = vec![
        format!("{}｜{}", RESULT.brand, evaluation.model.display_name),
        format!("{}：{}", verdict.kicker, verdict.headline),
        verdict.detail,
        format!("节省判断：{}", story.text()),
        result_ratio_text(evaluation.effective_ratio),
        result_net_text_for(evaluation),
    ];
    let injectable = evaluation
        .injection
        .as_ref()
        .map(|plan| !plan.system_prompt.trim().is_empty())
        .unwrap_or(false);
    if injectable {
        lines.push(format!(
            "{}：{}",
            RESULT.suffix_label,
            prompt_suffix(&evaluation.routing)
        ));
    }
    lines.push(format!("{}：{}", RESULT.time_label, generated_at));
    lines.join("\n")
}

pub const SAVING_TITLE: &str = "费用明细";

pub struct SavingMetrics {
    pub gross: &'static str,
    pub overhead: &'static str,
    pub net: &'static str,
    pub net_potential: &'static str,
    pub threshold: &'static str,
}

pub const SAVING_METRICS: SavingMetrics = SavingMetrics {
    gross: "切换可省",
    overhead: "切换要付",
    net: "切换后净省",
    net_potential: "净省潜力",
    threshold: "值得切的门槛",
};

pub const R_ROW_LABEL: &str = "一次回答大概思考多少 Token";
pub const R_LIVE_NOTE: &str = "换一档思考量，结论与门槛会立刻重算";

pub fn r_preset_aria(value: f64) -> String {
    format!("预期思考量 {}", js_num(value))
}
pub const GATE_NET: &str = "净收益";
pub const GATE_THRESHOLD: &str = "门槛";

pub fn r_auto_label(r: f64) -> String {
    format!("按这个问题估 {}", js_num(r))
}

pub const R_PRESETS: [f64; 3] = [800.0, 1200.0, 3000.0];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tone {
    Ok,
    Warn,
    Muted,
}

impl Tone {
    pub fn as_str(self) -> &'static str {
        match self {
            Tone::Ok => "ok",
            Tone::Warn => "warn",
            Tone::Muted => "muted",
        }
    }
}

pub struct Applicability {
    pub label: &'static str,
    pub tone: Tone,
}

pub fn applicability(evaluation: &Evaluation) -> Applicability {
    let model = &evaluation.model;
    let routing = &evaluation.routing;
    let savings = evaluation.savings.as_ref();
    let builtin = evaluation
        .profile
        .as_ref()
        .map(|p| p.measurement.method == ProfileSource::Builtin)
        .unwrap_or(false);
    let status = &evaluation.profile_status;

    if !model.reasoning_capable {
        return Applicability {
            label: "用不上：没有独立思考",
            tone: Tone::Muted,
        };
    }
    if routing.cost_status == CostStatus::BlockedQuality {
        return Applicability {
            label: "由质量决定，不看成本",
            tone: Tone::Muted,
        };
    }
    let Some(s) = savings else {
        if follows_input(evaluation) {
            return Applicability {
                label: "无需切换",
                tone: Tone::Muted,
            };
        }
        if status.health == ProfileHealth::Stale {
            return Applicability {
                label: if builtin {
                    "参考数据已过期"
                } else {
                    "测量已过期"
                },
                tone: Tone::Warn,
            };
        }
        if status.health == ProfileHealth::LowQuality {
            return Applicability {
                label: if builtin {
                    "参考数据质量不够"
                } else {
                    "测量质量不够"
                },
                tone: Tone::Warn,
            };
        }
        if status.health == ProfileHealth::None {
            return Applicability {
                label: if builtin {
                    "还没载入参考数据"
                } else {
                    "还没测过"
                },
                tone: Tone::Warn,
            };
        }
        return Applicability {
            label: "还不能计算",
            tone: Tone::Warn,
        };
    };
    if s.current == s.target {
        return Applicability {
            label: "无需切换",
            tone: Tone::Muted,
        };
    }
    if s.switchable {
        return Applicability {
            label: if builtin {
                "切换能省（参考）"
            } else {
                "切换能省"
            },
            tone: Tone::Ok,
        };
    }
    if s.net <= 0.0 {
        return Applicability {
            label: "切换反而更贵",
            tone: Tone::Warn,
        };
    }
    Applicability {
        label: "省得太少，不建议",
        tone: Tone::Warn,
    }
}

pub fn savings_blocked_notice(evaluation: &Evaluation) -> String {
    let routing = &evaluation.routing;
    let status = &evaluation.profile_status;
    let builtin = evaluation
        .profile
        .as_ref()
        .map(|p| p.measurement.method == ProfileSource::Builtin)
        .unwrap_or(false);
    if routing.cost_status == CostStatus::BlockedQuality {
        return "这类内容由质量决定，不看成本，所以没有费用对比。".to_string();
    }
    if routing.cost_status == CostStatus::NotApplicable {
        return "这个模型没有独立思考，用不上这工具。".to_string();
    }
    if status.health == ProfileHealth::Stale {
        return if builtin {
            format!(
                "参考数据已超 {} 天有效期，费用估算停用，重新载入恢复。",
                PROFILE_MAX_AGE_DAYS
            )
        } else {
            format!(
                "测量已超 {} 天有效期，费用停用，重测恢复。",
                PROFILE_MAX_AGE_DAYS
            )
        };
    }
    if status.health == ProfileHealth::LowQuality {
        return if builtin {
            "参考数据置信度不足，不参与费用估算。".to_string()
        } else {
            "这次测量置信度不足，不出费用结论，重测恢复。".to_string()
        };
    }
    if routing.cost_status == CostStatus::Ok {
        return format!(
            "按{}推理就是默认行为，不用切换，也没有注入开销。",
            lang_text(routing.decided_lang)
        );
    }
    "还没有可用的参考数据，算不出费用。先点上方「载入参考数据」。".to_string()
}

pub fn threshold_text(evaluation: &Evaluation) -> String {
    let (tokens, pct) = match evaluation.profile.as_ref() {
        Some(p) => (
            p.policy.switch_threshold_tokens,
            p.policy.switch_threshold_pct,
        ),
        None => (
            SWITCH_THRESHOLD_TOKENS_DEFAULT,
            SWITCH_THRESHOLD_PCT_DEFAULT,
        ),
    };
    format!("max({}, {}% × R)", js_num(tokens), round_i(pct * 100.0))
}

pub fn formula_lines(evaluation: &Evaluation) -> Vec<String> {
    let Some(s) = evaluation.savings.as_ref() else {
        return Vec::new();
    };
    let inj = evaluation
        .injection
        .as_ref()
        .map(|i| i.state != InjectionState::Disabled)
        .unwrap_or(false);
    let mut lines = vec![
        format!(
            "理论节省 = R × (1 − 成本倍数) = {} × (1 − {}) = {}",
            js_num(s.r),
            fmt3(s.multiplier),
            js_num(s.gross)
        ),
        format!(
            "成本倍数 = {}",
            if s.current == s.target {
                "1（无需切换）".to_string()
            } else if s.target == Lang::Zh {
                format!("ratio {}", fmt2(s.ratio))
            } else {
                format!("1 / ratio = {}", fmt3(s.multiplier))
            }
        ),
        if inj {
            let plan = evaluation.injection.as_ref().unwrap();
            format!(
                "切换要付 = system {} + 标签 {} = {}",
                js_num(plan.prompt_tokens),
                js_num(plan.tag_tokens),
                js_num(s.overhead)
            )
        } else {
            format!(
                "切换要付 = {}（语言约束系统提示 + 可选标签；不换语言就不会发生）",
                js_num(s.overhead)
            )
        },
        format!(
            "值得切的门槛 = {} = {}",
            threshold_text(evaluation),
            js_num(s.threshold)
        ),
    ];
    lines.shrink_to_fit();
    lines
}

pub const INJECT_STATE_LABEL: &str = "注入状态";

pub fn inject_reason_suffix(reason: &str) -> String {
    format!("（{}）", reason)
}

pub struct ComparisonCopy {
    pub title: &'static str,
    pub head: [&'static str; 4],
    pub chosen_chip: &'static str,
}

pub const COMPARISON: ComparisonCopy = ComparisonCopy {
    title: "三种做法比一比",
    head: ["做法", "说明", "预计思考 Token", "和同语言做法相比"],
    chosen_chip: "本工具",
};

pub struct ComparisonRow {
    pub id: &'static str,
    pub label: &'static str,
    pub detail: String,
    pub tokens: f64,
    pub delta: String,
    pub chosen: bool,
}

pub struct ComparisonView {
    pub meta: String,
    pub rows: Vec<ComparisonRow>,
    pub foot: &'static str,
}

fn delta_text(delta: f64, base: f64) -> String {
    if base <= 0.0 {
        return "不适用".to_string();
    }
    let pct = (delta / base * 100.0).round();
    if delta == 0.0 {
        return "持平".to_string();
    }
    format!("{}{}%", if delta > 0.0 { "+" } else { "" }, js_num(pct))
}

pub fn comparison_view(evaluation: &Evaluation) -> Option<ComparisonView> {
    let signals = &evaluation.signals;
    let routing = &evaluation.routing;
    let savings = evaluation.savings.as_ref()?;
    let injection = evaluation.injection.as_ref();
    let ratio = evaluation
        .profile
        .as_ref()
        .map(|p| p.measurement.effective_ratio)?;
    if ratio <= 0.0 {
        return None;
    }

    let cost = |l: Lang| if l == Lang::Zh { ratio } else { 1.0 };
    let tokens_for = |l: Lang| ((savings.r * cost(l)) / cost(savings.current)).round();
    let overhead = injection
        .filter(|i| i.state != InjectionState::Disabled)
        .map(|i| i.prompt_tokens + i.tag_tokens)
        .unwrap_or(0.0);
    let follow = tokens_for(signals.lang);
    let fixed_zh = tokens_for(Lang::Zh);
    let bridge = tokens_for(routing.decided_lang) + overhead;
    let mk = |id: &'static str, label: &'static str, detail: String, tokens: f64, chosen: bool| {
        ComparisonRow {
            id,
            label,
            detail,
            tokens,
            delta: delta_text(tokens - follow, follow),
            chosen,
        }
    };

    Some(ComparisonView {
        meta: format!(
            "按 {} Token（{}基准）估算 · 中英成本差 {} 倍",
            js_num(savings.r),
            lang_text(savings.current),
            fmt2(ratio)
        ),
        rows: vec![
            mk(
                "follow",
                "和提问同语言思考",
                format!("思考用{}", lang_text(signals.lang)),
                follow,
                false,
            ),
            mk(
                "fixed-zh",
                "永远用中文思考",
                "不分内容和模型".to_string(),
                fixed_zh,
                false,
            ),
            mk(
                "bridge",
                "LangBridge 的建议",
                format!(
                    "思考用{}{}",
                    lang_text(routing.decided_lang),
                    if overhead > 0.0 {
                        format!(" · 额外说明 {} Token", js_num(overhead))
                    } else {
                        " · 无需额外说明".to_string()
                    }
                ),
                bridge,
                true,
            ),
        ],
        foot: "口径：成本差 = 中文思考 Token ÷ 英文思考 Token。三行同一问题、同一成本差，只是思考语言不同。属同口径估算，不等于真实账单。",
    })
}

pub fn comparison_blocked_reason(evaluation: &Evaluation) -> String {
    let status = &evaluation.profile_status;
    let builtin = evaluation
        .profile
        .as_ref()
        .map(|p| p.measurement.method == ProfileSource::Builtin)
        .unwrap_or(false);
    let who = if builtin {
        "内置参考数据"
    } else {
        "测量"
    };
    if status.health == ProfileHealth::Stale {
        return format!(
            "{}已超 {} 天有效期，费用停用，没有三路对比。",
            who, PROFILE_MAX_AGE_DAYS
        );
    }
    if status.health == ProfileHealth::LowQuality {
        return format!("{}置信度不足，不参与费用对比。", who);
    }
    "还没有可用的参考数据，算不出费用，没有对比。".to_string()
}

pub struct CalMetricsCopy {
    pub r2: &'static str,
    pub linear: &'static str,
    pub linear_ok: &'static str,
    pub confidence: &'static str,
    pub ratio: &'static str,
}

impl CalMetricsCopy {
    pub fn linear_bad(&self) -> String {
        format!("偏差 >{}%", round_i(LINEAR_MAX_DEVIATION * 100.0))
    }
}

pub struct CalCopy {
    pub title: &'static str,
    pub sample: &'static str,
    pub zh: &'static str,
    pub en: &'static str,
    pub pending: &'static str,
    pub row_zh: &'static str,
    pub row_en: &'static str,
    pub diff_zh: &'static str,
    pub diff_en: &'static str,
    pub key_row: &'static str,
    pub metrics: CalMetricsCopy,
    pub badge_builtin: &'static str,
    pub badge_done: &'static str,
    pub rerun: &'static str,
    pub gen_at: &'static str,
    pub measured_at: &'static str,
    pub days_ago: &'static str,
    pub tokenizer_label: &'static str,
    pub proxy_badge: &'static str,
    pub sha_label: &'static str,
    pub warn_prefix: &'static str,
    pub rerun_stable: &'static str,
    pub rerun_unstable: &'static str,
    pub repro_summary: &'static str,
    pub method_note: &'static str,
    pub error_title: &'static str,
    pub last_run_failed: &'static str,
    pub api_retry_hint: &'static str,
}

pub const CAL: CalCopy = CalCopy {
    title: "测量与参考数据",
    sample: "样本",
    zh: "中文",
    en: "英文",
    pending: "待测",
    row_zh: "中文 T",
    row_en: "英文 T",
    diff_zh: "差分 Δ 中文",
    diff_en: "差分 Δ 英文",
    key_row: "中英文成本比",
    metrics: CalMetricsCopy {
        r2: "R²",
        linear: "线性度",
        linear_ok: "通过",
        confidence: "置信度",
        ratio: "采用比值",
    },
    badge_builtin: "内置参考",
    badge_done: "已完成",
    rerun: "再测一次（验证一致性）",
    gen_at: "参考数据生成于",
    measured_at: "测量于",
    days_ago: "天前",
    tokenizer_label: "tokenizer:",
    proxy_badge: "同系列代算",
    sha_label: "tokenizer sha256:",
    warn_prefix: "质量告警：",
    rerun_stable: "结果稳定",
    rerun_unstable: "结果不稳定，建议重试",
    repro_summary: "复现命令与原始计数",
    method_note: "比值由中英三档长度的计数斜率拟合得出，固定开销相互抵消，可跨模型比较。",
    error_title: "测量失败",
    last_run_failed: "本次测量没有完成，下面仍是上一次的记录：",
    api_retry_hint: "请在页面上方的「API 真实探测」卡片里修改参数后重试。",
};

impl CalCopy {
    pub fn hint(&self) -> String {
        format!("{} 档长度对比", SCALES.len())
    }

    pub fn intro(&self) -> String {
        format!(
            "同样的内容写成中英两版，各按 {} 三档测 Token；相邻两档相减，固定的开头结尾抵消，剩下的就是文字本身的成本差。",
            scales_txt()
        )
    }

    pub fn progress(&self) -> String {
        format!("正在生成 {} 组计数…", SCALES.len() * 2)
    }

    pub fn progress_aria(&self, stage: &str) -> String {
        format!("测量进度：{}", stage)
    }

    pub fn diff_line(&self, d1: f64, d2: f64, ratio: &str) -> String {
        format!(
            "Δ1 = {} · Δ2 = {} · Δ2/Δ1 = {}",
            js_num(d1),
            js_num(d2),
            ratio
        )
    }

    pub fn ratio_expr_prefix(&self, zh: &str, en: &str) -> String {
        format!("Δ中文 / Δ英文 = {} / {} = ", zh, en)
    }

    pub fn raw_observation(&self, sample_id: &str, tokens: f64, n: f64) -> String {
        format!("{}: {} Token（n={}）", sample_id, js_num(tokens), js_num(n))
    }

    pub fn offset_note(&self, a_zh: f64, a_en: f64) -> String {
        format!(
            "（固定开销 a_zh={} / a_en={} 已抵消）",
            js_num(a_zh),
            js_num(a_en)
        )
    }

    pub fn rerun_deviation(&self, pct: f64) -> String {
        format!("复测一致性：偏差 {}% · ", js_num(pct))
    }

    pub fn repro_cmd(&self, model_id: &str) -> String {
        format!(
            "cargo run -p langbridge-cli -- calibrate --model {}",
            model_id
        )
    }

    pub fn repro_note(&self, name: &str, version: &str) -> String {
        format!(
            "计数来自 {}（修订 {}）的真实 tokenizer 编码；服务端计费口径可能仍有差异，可用 API 探测复核。",
            name, version
        )
    }
}

pub struct ProfileCopy {
    pub title: &'static str,
    pub migrate_chip: &'static str,
    pub empty_notice: &'static str,
    pub import_fail_title: &'static str,
    pub import_fail_msg: &'static str,
    pub import_ok: &'static str,
    pub read_fail: &'static str,
    pub delete_confirm: &'static str,
    pub expired_notice_measured: &'static str,
}

pub static PROFILE: std::sync::LazyLock<ProfileCopy> = std::sync::LazyLock::new(|| ProfileCopy {
    title: "参考与测量记录",
    migrate_chip: "已从 v2 迁移",
    empty_notice:
        "还没有记录。点上方「载入参考数据」或「测一下」后会生成可复现的 profile.json，刷新仍保留。",
    import_fail_title: "导入失败",
    import_fail_msg: "文件未通过画像校验，未写入本地存储。",
    import_ok: "画像已导入，结论与费用已更新。",
    read_fail: "读取文件失败",
    delete_confirm: "确认删除这条记录？",
    expired_notice_measured: "测量太久远了：重新测一次之前，不会给出费用结论。",
});

pub struct EvidenceCopy {
    pub gen_at: &'static str,
    pub covered_summary: &'static str,
    pub covered_ok: &'static str,
    pub covered_miss: &'static str,
    pub current_mark: &'static str,
    pub repro_summary: &'static str,
    pub repro_smoke: &'static str,
    pub sha_label: &'static str,
    pub exact_badge: &'static str,
    pub limit_summary: &'static str,
    pub canonical_summary: &'static str,
    pub record_summary: &'static str,
}

impl EvidenceCopy {
    pub fn tokenizer_mixed(&self) -> &'static str {
        if evidence_meta()
            .tokenizers
            .iter()
            .any(|entry| entry.1.exact == Some(false))
        {
            "按各模型公开 tokenizer 计数（含同系列代算）"
        } else {
            "按各模型公开 tokenizer 计数"
        }
    }

    pub fn repro_note(&self) -> String {
        format!(
            "样本由固定开头和可重复的正文组成；按 {} 三档共用同一个开头，相减后固定开销就抵消了。",
            scales_txt()
        )
    }
}

pub const EVIDENCE: EvidenceCopy = EvidenceCopy {
    gen_at: "证据生成于",
    covered_summary: "内置参考数据覆盖的模型",
    covered_ok: "当前模型有内置参考数据，不用 API Key 就能载入估算。",
    covered_miss: "当前模型没有内置数据：请换用被覆盖的模型，或使用「进阶设置」里的 API 测量。",
    current_mark: " ← 当前模型",
    repro_summary: "复现命令与样本",
    repro_smoke: "cargo run -p langbridge-cli -- doctor",
    sha_label: "样本集 sha256：",
    exact_badge: "同系列代算",
    limit_summary: "已知限制",
    canonical_summary: "Canonical Request（含三阶段遥测）",
    record_summary: "决策记录（DecisionRecord）",
};

pub fn limitations(covered_count: usize) -> Vec<String> {
    let meta = evidence_meta();
    let mut tokenizers: Vec<String> = Vec::new();
    for (_, tokenizer) in &meta.tokenizers {
        let text = format!("{}@{}", tokenizer.name, tokenizer.version);
        if !tokenizers.contains(&text) {
            tokenizers.push(text);
        }
    }
    let has_proxy = meta
        .tokenizers
        .iter()
        .any(|entry| entry.1.exact == Some(false));
    let mut items = vec![
        format!(
            "内置数据用各模型公开 tokenizer 对同一组样本计数：{}，覆盖 {} 个模型。这不是服务端账单，精确计费请用「API 测量」或 CLI。",
            tokenizers.join("、"),
            covered_count
        ),
        "节省量按预期思考量估算；实际任务长度差得多时结论会变，可在「费用明细」里调整。".to_string(),
        "成本之外还有质量与速度：本工具先保质量，再算成本。".to_string(),
        "流式与非流式注入方式不同：流式下不加 <thinking>/<answer> 标签。".to_string(),
    ];
    if has_proxy {
        items.insert(
            1,
            "标注「同系列代算」的模型没有公开 tokenizer，用同系列开源 tokenizer 代算。".to_string(),
        );
    }
    items
}

pub struct Preset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub model_id: &'static str,
}

pub struct ProbeCopy {
    pub title: &'static str,
    pub intro: &'static str,
    pub source_builtin: &'static str,
    pub source_api: &'static str,
    pub source_none: &'static str,
    pub provider: &'static str,
    pub provider_aria: &'static str,
    pub provider_custom: &'static str,
    pub api_key: &'static str,
    pub show_key: &'static str,
    pub hide_key: &'static str,
    pub endpoint_edit: &'static str,
    pub endpoint_collapse: &'static str,
    pub base_url: &'static str,
    pub base_url_placeholder: &'static str,
    pub model_id: &'static str,
    pub need_base_url: &'static str,
    pub need_model_id: &'static str,
    pub need_api_key: &'static str,
    pub ok_title: &'static str,
    pub compare_builtin: &'static str,
    pub compare_measured: &'static str,
    pub compare_no_baseline: &'static str,
    pub compare_updated: &'static str,
    pub compare_title: &'static str,
    pub presets: [Preset; 7],
    pub preparing: &'static str,
    pub finishing: &'static str,
    pub test_connect: &'static str,
    pub testing: &'static str,
    pub key_toggle_aria: &'static str,
    pub start_measure: &'static str,
    pub running_note: &'static str,
    pub connected: &'static str,
    pub connect_failed: &'static str,
    pub not_measured: &'static str,
    pub log_title: &'static str,
    pub cancel: &'static str,
    pub retry_run: &'static str,
}

pub const PROBE: ProbeCopy = ProbeCopy {
    title: "API 真实探测",
    intro: "公开数据未必等于你账号的计费口径。填自己的 Key 测一遍，整页结论和费用换成你的数。",
    source_builtin: "当前用内置参考",
    source_api: "当前用实测数据",
    source_none: "当前未测量",
    provider: "服务商",
    provider_aria: "选择服务商",
    provider_custom: "自定义",
    api_key: "API Key",
    show_key: "显示",
    hide_key: "隐藏",
    endpoint_edit: "地址与模型",
    endpoint_collapse: "收起",
    base_url: "Base URL",
    base_url_placeholder: "https://api.example.com/v1",
    model_id: "模型 ID",
    need_base_url: "请先填写 Base URL",
    need_model_id: "请填写模型 ID",
    need_api_key: "请先填写 API Key（本地服务可留空）",
    ok_title: "测量完成",
    compare_builtin: "内置",
    compare_measured: "实测",
    compare_no_baseline: "无参考",
    compare_updated: "整页结论与费用已改用实测值。",
    compare_title: "和内置参考比一比",
    presets: [
        Preset {
            name: "OpenAI",
            base_url: "https://api.openai.com/v1",
            model_id: "gpt-4o-mini",
        },
        Preset {
            name: "盘古 MaaS",
            base_url: "https://api.modelarts-maas.com/openai/v1",
            model_id: "openPangu-2.0-Flash",
        },
        Preset {
            name: "DeepSeek",
            base_url: "https://api.deepseek.com/v1",
            model_id: "deepseek-reasoner",
        },
        Preset {
            name: "Moonshot",
            base_url: "https://api.moonshot.cn/v1",
            model_id: "kimi-k2-0905-preview",
        },
        Preset {
            name: "通义千问",
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1",
            model_id: "qwen-plus",
        },
        Preset {
            name: "智谱",
            base_url: "https://open.bigmodel.cn/api/paas/v4",
            model_id: "glm-4.5",
        },
        Preset {
            name: "本地",
            base_url: "http://localhost:11434/v1",
            model_id: "llama3.1",
        },
    ],
    preparing: "准备中",
    finishing: "核对结果",
    test_connect: "测试连接",
    testing: "测试中",
    key_toggle_aria: "显示或隐藏 API Key",
    start_measure: "开始测量",
    running_note: "正在测量，进度见下方。过程中可以随时取消。",
    connected: "连接正常",
    connect_failed: "连接失败",
    not_measured: "未测",
    log_title: "本次记录",
    cancel: "取消测量",
    retry_run: "再测一次",
};

impl ProbeCopy {
    pub fn run_note(&self) -> String {
        format!(
            "共 {} 次请求，每次只输出 1 个 Token。地址与模型记在本机，Key 只留在这个页面。",
            SCALES.len() * 2
        )
    }

    pub fn fail_title(&self, code: &str) -> String {
        format!("测量失败（{}）", code)
    }

    pub fn compare_summary(&self, builtin: Option<&str>) -> String {
        match builtin {
            None => "该模型没有内置参考，测一次看你的实测值。".to_string(),
            Some(value) => format!("内置参考 {}×，测一次换成你的实测值。", value),
        }
    }

    pub fn progress_aria(&self, stage: &str) -> String {
        format!("探测进度：{}", stage)
    }

    pub fn step_label(&self, step: &ProbeStep) -> String {
        format!(
            "第 {}/{} 次 · {} N={}",
            step.index,
            step.total,
            lang_text(step.lang),
            js_num(step.n)
        )
    }

    pub fn progress_count(&self, done: usize, total: usize) -> String {
        format!("已完成 {}/{} 次", done, total)
    }

    pub fn log_ok(&self, lang_text_value: &str, n: f64, prompt_tokens: f64) -> String {
        format!(
            "{} N={}：{} Token",
            lang_text_value,
            js_num(n),
            js_num(prompt_tokens)
        )
    }

    pub fn log_retry(&self, attempt: u32, delay_ms: f64, message: &str) -> String {
        format!(
            "第 {} 次重试（{:.1} 秒后）：{}",
            attempt,
            delay_ms / 1000.0,
            message
        )
    }

    pub fn log_fail(&self, lang_text_value: &str, n: f64, message: &str) -> String {
        format!("{} N={} 失败：{}", lang_text_value, js_num(n), message)
    }

    pub fn log_cancelled(&self, done: usize, total: usize) -> String {
        format!("已取消：完成 {}/{} 次", done, total)
    }

    pub fn hint(&self, code: &str) -> &'static str {
        match code {
            "unauthorized" => "检查 Key 是否复制完整、是否已开通该模型；部分厂商的模型需要单独申请权限。",
            "bad_base_url" => "Base URL 一般以 /v1 结尾；已填 /chat/completions 也能识别。Azure 须填完整 deployments 地址并带 api-version。",
            "bad_model" => "模型 ID 必须与服务商文档完全一致，注意大小写和日期后缀。",
            "bad_request" => "请求参数被拒绝：已自动去掉服务端不支持的 temperature / max_tokens；若仍失败，换官方 Base URL 再试。",
            "bad_response" => "该地址返回了网页而不是 JSON：确认 Base URL 指向接口，而不是控制台首页。",
            "network_error" => "请求没有到达服务商：检查网络、地址拼写与协议（http/https）。",
            "cors_blocked" => "浏览器被服务商的跨域策略拦住了：本地已确认地址可达，但对方不允许网页直连。请改用命令行校准（langbridge calibrate --method api --base-url …），或在本地启动代理后把 Base URL 指向它。",
            "server_error" => "服务端临时故障：已自动重试过，稍等片刻再点一次。",
            "rate_limited" => "触发限流：稍等一会再试，并确认账号余额充足。",
            "timeout" => "请求迟迟没有响应：检查网络，或换一个更近的接口地址。",
            "usage_missing" => "该接口没有返回 usage.prompt_tokens：换官方接口，或改用命令行校准。",
            "cancelled" => "参数没有改动的话，可以直接再测一次。",
            "sample_too_small" => "返回的用量异常偏小：确认 Base URL 与模型是否匹配。",
            "low_fit" => "计数与文本长度不成比例：可能地址指向了另一个模型。",
            _ => "重试一次；如果仍失败，请把上面的错误信息保留下来。",
        }
    }
}

pub struct StorageCopy {
    pub unavailable: &'static str,
    pub profile_not_saved: &'static str,
    pub profile_imported_not_saved: &'static str,
}

impl StorageCopy {
    pub fn text_trimmed(&self, max: usize) -> String {
        format!(
            "输入过长，只保存了前 {} 字；刷新后不会恢复全文。",
            thousands(max as i64)
        )
    }
}

pub const STORAGE: StorageCopy = StorageCopy {
    unavailable: "浏览器存储不可用：画像与设置无法持久化",
    profile_not_saved:
        "画像已生成，但浏览器存储已满或不可用：刷新后会丢失，请先导出画像或清理过长的输入。",
    profile_imported_not_saved: "画像已导入，但浏览器存储不可用（刷新后会丢失）",
};

pub struct CrashCopy {
    pub title: &'static str,
    pub message: &'static str,
    pub reload: &'static str,
    pub clear: &'static str,
    pub clear_confirm: &'static str,
}

pub const CRASH: CrashCopy = CrashCopy {
    title: "页面遇到问题",
    message: "界面刚刚处理不了当前内容。可以先重新加载；本机保存的画像与设置不会被修改。",
    reload: "重新加载",
    clear: "清除本地记录并重载",
    clear_confirm: "确认清除本机保存的画像与设置？此操作不可撤销。",
};

pub struct AdvancedCopy {
    pub inject_title: &'static str,
    pub streaming: &'static str,
    pub system_prompt: &'static str,
    pub system_prompt_placeholder: &'static str,
    pub inject_state_label: &'static str,
    pub post_title: &'static str,
    pub post_tags: &'static str,
    pub post_polish_on: &'static str,
    pub post_polish_off: &'static str,
}

impl AdvancedCopy {
    pub fn post_expected(&self, l: Lang) -> String {
        format!(
            "漂移检测：期望输出为{}，语言纯度低于 {}% 判定漂移（不阻断）",
            lang_text(l),
            round_i(DRIFT_THRESHOLD * 100.0)
        )
    }

    pub fn post_terms(&self, n: usize) -> String {
        format!("术语一致性：{} 条中英术语，词边界匹配、跳过代码", n)
    }
}

pub const ADVANCED: AdvancedCopy = AdvancedCopy {
    inject_title: "怎么告诉模型换语言（注入协议）",
    streaming: "流式响应（流式下不注入 <thinking>/<answer> 标签）",
    system_prompt: "你已有的 system prompt（若已规定语言，将跳过注入）",
    system_prompt_placeholder: "留空表示未提供 system prompt",
    inject_state_label: "注入状态：",
    post_title: "输出后处理（Phase 3）",
    post_tags: "双标签解析：<thinking>/<answer> 成对剥离，无标签时全文为回答",
    post_polish_on: "意译润色：空白归一与直译腔清理已生效",
    post_polish_off: "意译润色：流式模式只统计不改写",
};

pub fn evidence_version_badge(version: u32) -> String {
    format!("数据 v{}", version)
}

pub fn evidence_count() -> usize {
    list_evidence_models().len()
}

pub fn evidence_generated_at() -> String {
    evidence_meta().generated_at
}

pub struct FlowCopy {
    pub empty_pick: &'static str,
    pub probe_steps: [&'static str; 4],
    pub probe_next_key: &'static str,
    pub probe_next_test: &'static str,
    pub probe_next_run: &'static str,
    pub probe_next_done: &'static str,
    pub probe_saved: &'static str,
    pub cal_lead: &'static str,
    pub cal_left: &'static str,
    pub cal_right: &'static str,
    pub cal_steps: [&'static str; 3],
    pub cal_current: &'static str,
    pub cal_no_data: &'static str,
    pub cal_go_probe: &'static str,
    pub cost_formula: &'static str,
    pub evidence_lead: &'static str,
    pub evidence_groups: [&'static str; 5],
    pub profile_note: &'static str,
    pub inject_lead: &'static str,
    pub inject_steps: [&'static str; 3],
    pub inject_step1: &'static str,
    pub inject_step2: &'static str,
    pub inject_step3: &'static str,
    pub inject_copy: &'static str,
    pub inject_copied: &'static str,
    pub inject_copy_fail: &'static str,
    pub inject_stream_note: &'static str,
    pub inject_conflict_note: &'static str,
}

pub const FLOW: FlowCopy = FlowCopy {
    empty_pick: "点一行，直接看这个模型的结论",
    probe_steps: ["选服务商", "填 API Key", "测试连接", "开始测量"],
    probe_next_key: "先填入 API Key。Key 只留在本页内存，不写入本地存储。",
    probe_next_test: "接着点「测试连接」，先确认地址与 Key 能通，再跑测量。",
    probe_next_run: "连接正常。现在点「开始测量」，会发 6 次请求，大约十几秒。",
    probe_next_done: "测量完成，已写入「我的测量」，并切换到该模型。",
    probe_saved: "已写入模型列表并切换到该模型",
    cal_lead: "这里只回答两件事：你选的模型用什么比值，这个比值又是怎么测出来的。",
    cal_left: "内置参考覆盖的模型",
    cal_right: "比值是怎么来的",
    cal_steps: [
        "同一段意思，中英各准备三档长度：N、2N、4N。",
        "每档真实计数一次，得到 6 个 token 数。",
        "同语言内相减，抵消固定开销，剩下的斜率就是每多一个字要多少 token。中文斜率除以英文斜率，就是成本比。",
    ],
    cal_current: "当前模型",
    cal_no_data: "当前模型还没有数据。可以载入同模型的内置参考，或到下方做一次真实测量。",
    cal_go_probe: "去 API 测量",
    cost_formula: "净收益等于毛收益减去注入开销。只有净收益超过门槛，才建议换语言。",
    evidence_lead: "这一区给想核对的人：数据从哪来、覆盖谁、能推到什么程度、怎么自己复现。",
    evidence_groups: [
        "数据从哪来",
        "覆盖哪些模型",
        "结论能用到什么程度",
        "自己复现",
        "我的模型画像",
    ],
    profile_note: "画像只存在本机，包含测量数据，不含 Key 与问题内容。换设备时先导出再导入。",
    inject_lead: "三步把结论用起来：先确认建议，再复制提示，最后放进你的调用。",
    inject_steps: ["确认建议", "复制提示", "放进调用"],
    inject_step1: "上方结论显示「建议中文推理」或「建议英文推理」时，才需要复制。显示不建议切换时，这一步可以跳过。",
    inject_step2: "点下方复制按钮。提示里只有语言约束，不含你的问题内容。",
    inject_step3: "放进 system prompt，或用户消息末尾。用 CLI 或本地代理时，这一步会自动完成。",
    inject_copy: "复制这段提示",
    inject_copied: "已复制",
    inject_copy_fail: "复制失败",
    inject_stream_note: "流式请求不会注入标签，只保留语言指令，避免破坏流式解析。",
    inject_conflict_note: "如果你已经在系统提示里写了语言要求，这里会判为冲突并跳过注入。",
};
