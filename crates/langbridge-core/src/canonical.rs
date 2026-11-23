//! 规范化请求与决策遥测的构造。只做字段映射，不校验输入，调用方保证数据完整。

use crate::types::{
    CanonicalRequest, DecisionRecord, DecisionSummary, InjectionPlan, InjectionState, ProfileRef,
    Routing, SavingsResult, Signals, Telemetry,
};

impl crate::types::DecidedBy {
    pub fn as_str(self) -> &'static str {
        match self {
            crate::types::DecidedBy::L1 => "L1",
            crate::types::DecidedBy::L2 => "L2",
            crate::types::DecidedBy::L3 => "L3",
            crate::types::DecidedBy::L4 => "L4",
            crate::types::DecidedBy::L4Blocked => "L4-blocked",
            crate::types::DecidedBy::Fallback => "fallback",
        }
    }
}

fn layer_label(layer: crate::types::RoutingLayer) -> &'static str {
    match layer {
        crate::types::RoutingLayer::L1 => "L1",
        crate::types::RoutingLayer::L2 => "L2",
        crate::types::RoutingLayer::L3 => "L3",
        crate::types::RoutingLayer::L4 => "L4",
    }
}

#[derive(Debug, Clone)]
pub struct CanonicalInput<'a> {
    pub raw_input: &'a str,
    pub signals: Signals,
    pub routing: Routing,
    pub injection: Option<InjectionPlan>,
    pub savings: Option<SavingsResult>,
    pub final_output: Option<String>,
    pub request_id: String,
    pub created_at: String,
}

/// transformed_input 与 raw_input 相同：规范化不改写输入。
pub fn build_canonical_request(input: CanonicalInput<'_>) -> CanonicalRequest {
    let telemetry = Telemetry {
        request_id: input.request_id.clone(),
        detected_lang: input.signals.lang,
        reasoning_lang: input.routing.decided_lang,
        output_lang: input.routing.output_lang,
        task: input.signals.task,
        decided_by: input.routing.decided_by,
        cost_status: input.routing.cost_status,
        reasons: input
            .routing
            .steps
            .iter()
            .filter(|s| s.hit || s.terminated)
            .map(|s| format!("{}: {}", layer_label(s.layer), s.reason))
            .collect(),
        decision_chain: input.routing.steps.clone(),
        has_code: input.signals.has_code,
        input_tokens_est: input.signals.input_tokens_est,
        expected_reasoning_tokens: input.savings.as_ref().map(|s| s.r),
        gross_saved_tokens: input.savings.as_ref().map(|s| s.gross),
        overhead_tokens: input.savings.as_ref().map(|s| s.overhead),
        net_saved_tokens: input.savings.as_ref().map(|s| s.net),
        injection: input.injection.as_ref().map(|i| i.state),
        created_at: input.created_at,
    };
    CanonicalRequest {
        request_id: input.request_id,
        raw_input: input.raw_input.to_string(),
        signals: input.signals,
        routing: input.routing,
        system_prompt: input
            .injection
            .as_ref()
            .map(|i| i.system_prompt.clone())
            .unwrap_or_default(),
        transformed_input: input.raw_input.to_string(),
        final_output: input.final_output.unwrap_or_default(),
        telemetry_json: telemetry,
    }
}

pub fn build_decision_record(
    model_id: &str,
    signals: Signals,
    routing: Routing,
    savings: Option<SavingsResult>,
    injection: Option<InjectionPlan>,
    profile: ProfileRef,
    created_at: String,
) -> DecisionRecord {
    DecisionRecord {
        schema: "langbridge.decision".to_string(),
        version: 1,
        created_at,
        model: model_id.to_string(),
        profile,
        signals,
        routing,
        savings,
        injection,
    }
}

/// 脱敏摘要，不含原文、Key 与响应正文。
pub fn summarize_decision(
    model_id: &str,
    signals: &Signals,
    routing: &Routing,
    savings: Option<&SavingsResult>,
    injection: Option<&InjectionPlan>,
    duration_ms: Option<u64>,
    now_iso: &str,
) -> DecisionSummary {
    DecisionSummary {
        v: 3,
        ts: now_iso.to_string(),
        model_id: model_id.to_string(),
        task_type: signals.task,
        input_lang: signals.lang,
        reasoning_lang: routing.decided_lang,
        output_lang: routing.output_lang,
        decided_by: routing.decided_by.as_str().to_string(),
        cost_status: routing.cost_status,
        effective_ratio: savings.map(|s| s.ratio),
        expected_reasoning_tokens: savings.map(|s| s.r).unwrap_or(0.0),
        gross_saved_tokens: savings.map(|s| s.gross).unwrap_or(0.0),
        overhead_tokens: savings.map(|s| s.overhead).unwrap_or(0.0),
        net_saved_tokens: savings.map(|s| s.net).unwrap_or(0.0),
        inject: injection.map(|i| i.state).unwrap_or(InjectionState::Disabled),
        duration_ms,
    }
}
