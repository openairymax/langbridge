//! 核心数据契约。纯类型，不含 DOM 与平台 API，供 Web、CLI 与代理共用。

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Zh,
    En,
}

impl Lang {
    pub fn name_zh(self) -> &'static str {
        match self {
            Lang::Zh => "中文",
            Lang::En => "英文",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskType {
    Code,
    Math,
    Culture,
    Policy,
    General,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostStatus {
    Ok,
    BlockedUncalibrated,
    NotApplicable,
    BlockedQuality,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProfileSource {
    Builtin,
    ApiProbe,
    LiveRun,
    Manual,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Qwen,
    Deepseek,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Warning {
    LowFit,
    SampleTooSmall,
    Outlier,
    ProbeDegraded,
}

/// 语言信号。质量单位统一：中文按字符数，英文按字符数除以 4.5。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Signals {
    pub lang: Lang,
    pub lang_confidence: f64,
    pub mixed: bool,
    pub zh_mass: f64,
    pub en_mass: f64,
    pub task: TaskType,
    pub has_code: bool,
    pub has_math: bool,
    pub input_chars: usize,
    pub input_tokens_est: u32,
    pub history_tokens_est: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelMeta {
    pub model_id: String,
    pub display_name: String,
    pub family: Family,
    pub native_lang: Option<Lang>,
    pub reasoning_capable: bool,
    pub context_limit: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Observation {
    #[serde(default)]
    pub sample_id: String,
    pub lang: Lang,
    pub n: f64,
    pub prompt_tokens: f64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TokenizerMeta {
    pub name: String,
    pub version: String,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exact: Option<bool>,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
pub struct Pair {
    pub zh: f64,
    pub en: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResult {
    pub density_ratio: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub generation_ratio: Option<f64>,
    pub effective_ratio: f64,
    pub slopes: Pair,
    pub fixed_overhead: Pair,
    pub r2: f64,
    pub linearity_ok: bool,
    pub confidence: f64,
    pub observations: Vec<Observation>,
    pub method: ProfileSource,
    pub tokenizer: TokenizerMeta,
    pub measured_at: String,
    pub warnings: Vec<Warning>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Policy {
    pub mode: PolicyMode,
    pub switch_threshold_tokens: f64,
    pub switch_threshold_pct: f64,
    pub ratio_high: f64,
    pub ratio_low: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PolicyMode {
    Balanced,
    Quality,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub source: ProfileSource,
    pub reproduce: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub migrated: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelProfile {
    pub schema: String,
    pub version: u32,
    pub model: ModelMeta,
    pub measurement: CalibrationResult,
    pub policy: Policy,
    pub provenance: Provenance,
}

impl ModelProfile {
    pub const SCHEMA: &'static str = "langbridge.profile";
    pub const VERSION: u32 = 3;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RoutingLayer {
    L1,
    L2,
    L3,
    L4,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutingStep {
    pub layer: RoutingLayer,
    pub hit: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decided: Option<Lang>,
    pub reason: String,
    pub confidence: f64,
    pub terminated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DecidedBy {
    #[serde(rename = "L1")]
    L1,
    #[serde(rename = "L2")]
    L2,
    #[serde(rename = "L3")]
    L3,
    #[serde(rename = "L4")]
    L4,
    #[serde(rename = "L4-blocked")]
    L4Blocked,
    #[serde(rename = "fallback")]
    Fallback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Routing {
    pub decided_lang: Lang,
    pub decided_by: DecidedBy,
    pub output_lang: Lang,
    pub quality_first: bool,
    pub cost_status: CostStatus,
    pub steps: Vec<RoutingStep>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavingsResult {
    #[serde(rename = "R")]
    pub r: f64,
    pub current: Lang,
    pub target: Lang,
    pub ratio: f64,
    pub multiplier: f64,
    pub gross: f64,
    pub overhead: f64,
    pub net: f64,
    pub threshold: f64,
    pub switchable: bool,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InjectionState {
    Injected,
    Appended,
    SkippedConflict,
    Disabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InjectionPlan {
    pub state: InjectionState,
    pub target_lang: Lang,
    pub system_prompt: String,
    pub prompt_tokens: f64,
    pub tag_tokens: f64,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Telemetry {
    pub request_id: String,
    pub detected_lang: Lang,
    pub reasoning_lang: Lang,
    pub output_lang: Lang,
    pub task: TaskType,
    pub decided_by: DecidedBy,
    pub cost_status: CostStatus,
    pub reasons: Vec<String>,
    pub decision_chain: Vec<RoutingStep>,
    pub has_code: bool,
    pub input_tokens_est: u32,
    pub expected_reasoning_tokens: Option<f64>,
    pub gross_saved_tokens: Option<f64>,
    pub overhead_tokens: Option<f64>,
    pub net_saved_tokens: Option<f64>,
    pub injection: Option<InjectionState>,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalRequest {
    pub request_id: String,
    pub raw_input: String,
    pub signals: Signals,
    pub routing: Routing,
    pub system_prompt: String,
    pub transformed_input: String,
    pub final_output: String,
    pub telemetry_json: Telemetry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileRef {
    pub version: Option<u32>,
    pub source: Option<ProfileSource>,
    pub measured_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub schema: String,
    pub version: u32,
    pub created_at: String,
    pub model: String,
    pub profile: ProfileRef,
    pub signals: Signals,
    pub routing: Routing,
    pub savings: Option<SavingsResult>,
    pub injection: Option<InjectionPlan>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionSummary {
    pub v: u32,
    pub ts: String,
    pub model_id: String,
    pub task_type: TaskType,
    pub input_lang: Lang,
    pub reasoning_lang: Lang,
    pub output_lang: Lang,
    pub decided_by: String,
    pub cost_status: CostStatus,
    pub effective_ratio: Option<f64>,
    pub expected_reasoning_tokens: f64,
    pub gross_saved_tokens: f64,
    pub overhead_tokens: f64,
    pub net_saved_tokens: f64,
    pub inject: InjectionState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    EmptyInput,
    UnknownModel,
    NoEvidence,
    UsageMissing,
    LowFit,
    SampleTooSmall,
    Outlier,
    InvalidProfile,
    StorageFull,
    StorageUnavailable,
    CorsBlocked,
    Unauthorized,
    RateLimited,
    Timeout,
    ExpiredProfile,
    BadBaseUrl,
    BadModel,
    BadRequest,
    BadResponse,
    NetworkError,
    ServerError,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldError {
    pub path: String,
    pub message: String,
}

/// 引擎统一错误，包含错误码与可直接展示的中文说明。
#[derive(Debug, Clone)]
pub struct EngineError {
    pub code: ErrorCode,
    pub message: String,
}

impl EngineError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for EngineError {}

pub type EngineResult<T> = Result<T, EngineError>;
