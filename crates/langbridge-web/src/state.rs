//! Web 状态层：唯一的应用状态、派生评测与动作入口。

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use langbridge_core::calibrate::{calibration_usable, compare_rerun};
use langbridge_core::canonical::{build_canonical_request, build_decision_record, CanonicalInput};
use langbridge_core::evidence::calibrate_from_evidence;
use langbridge_core::profile_store::{
    age_days, parse_profile_file, profile_from_calibration, serialize_profile, ParseProfileResult,
};
use langbridge_core::profiles::meta_for;
use langbridge_core::router::{is_profile_expired, route, RouteInput};
use langbridge_core::savings::estimate_r;
use langbridge_core::signals::extract_signals;
use langbridge_core::types::{
    CalibrationResult, CanonicalRequest, DecisionRecord, EngineError, FieldError, InjectionPlan,
    Lang, ModelMeta, ModelProfile, ProfileRef, ProfileSource, Routing, SavingsResult, Signals,
};

use leptos::prelude::*;

use crate::copy::{lang_text, PROBE, STORAGE};
use crate::io;
use crate::probe::{calibrate_via_api, test_connection, ConnectionCheck, ProbeEvent, ProbeOptions};

pub const UI_KEY: &str = "langbridge.ui.v3";
pub const PROFILE_KEY: &str = "langbridge.profile.v3";
pub const MEASURED_KEY: &str = "langbridge.measured.v3";
const MEASURED_LIMIT: usize = 12;
pub const MAX_PERSISTED_TEXT: usize = 100_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProfileHealth {
    None,
    Usable,
    Stale,
    LowQuality,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProfileStatus {
    pub health: ProfileHealth,
    pub usable: bool,
    pub age_days: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Evaluation {
    pub model: ModelMeta,
    pub profile: Option<ModelProfile>,
    pub profile_status: ProfileStatus,
    pub signals: Signals,
    pub r: f64,
    pub routing: Routing,
    pub savings: Option<SavingsResult>,
    pub injection: Option<InjectionPlan>,
    pub effective_ratio: Option<f64>,
    pub confidence: Option<f64>,
}

impl PartialEq for Evaluation {
    fn eq(&self, other: &Self) -> bool {
        self.model.model_id == other.model.model_id
            && self.signals.lang == other.signals.lang
            && self.signals.task == other.signals.task
            && self.signals.mixed == other.signals.mixed
            && self.signals.input_chars == other.signals.input_chars
            && self.r == other.r
            && self.routing.decided_lang == other.routing.decided_lang
            && self.routing.decided_by == other.routing.decided_by
            && self.routing.cost_status == other.routing.cost_status
            && self.routing.quality_first == other.routing.quality_first
            && self.profile_status == other.profile_status
            && self.effective_ratio == other.effective_ratio
            && self.profile.as_ref().map(profile_stamp) == other.profile.as_ref().map(profile_stamp)
            && self.savings.as_ref().map(|s| s.net) == other.savings.as_ref().map(|s| s.net)
    }
}

fn profile_stamp(profile: &ModelProfile) -> (u32, String, Option<ProfileSource>) {
    (
        profile.version,
        profile.measurement.measured_at.clone(),
        Some(profile.measurement.method),
    )
}

pub fn profile_status(profile: Option<&ModelProfile>, now: i64) -> ProfileStatus {
    let Some(profile) = profile else {
        return ProfileStatus {
            health: ProfileHealth::None,
            usable: false,
            age_days: None,
        };
    };
    let days = {
        let raw = age_days(profile, now);
        if raw.is_finite() {
            Some(raw.max(0.0).floor() as i64)
        } else {
            None
        }
    };
    let builtin = profile.measurement.method == ProfileSource::Builtin;
    if !builtin && is_profile_expired(profile, now) {
        return ProfileStatus {
            health: ProfileHealth::Stale,
            usable: false,
            age_days: days,
        };
    }
    if !calibration_usable(&profile.measurement) {
        return ProfileStatus {
            health: ProfileHealth::LowQuality,
            usable: false,
            age_days: days,
        };
    }
    ProfileStatus {
        health: ProfileHealth::Usable,
        usable: true,
        age_days: days,
    }
}

pub fn select_model(state: &AppState) -> ModelMeta {
    meta_for(
        &state.model_id,
        state.profile.as_ref().map(|p| p.measurement.effective_ratio),
    )
}

pub fn select_evaluation(state: &AppState, now: i64) -> Evaluation {
    let model = select_model(state);
    let signals = state
        .signals
        .clone()
        .unwrap_or_else(|| extract_signals(&state.text, ""));
    let profile = state
        .profile
        .clone()
        .filter(|p| p.model.model_id == model.model_id);
    let r = state.r_override.unwrap_or_else(|| estimate_r(signals.task, &model));
    let result = route(&RouteInput {
        signals: &signals,
        model: &model,
        profile: profile.as_ref(),
        r,
        streaming: state.streaming,
        user_system_prompt: Some(&state.user_system_prompt),
        now_ms: now,
    });
    let injection = Some(result.injection.clone());
    let status = profile_status(profile.as_ref(), now);
    Evaluation {
        model,
        effective_ratio: profile.as_ref().map(|p| p.measurement.effective_ratio),
        confidence: profile.as_ref().map(|p| p.measurement.confidence),
        profile,
        profile_status: status,
        signals,
        r,
        routing: result.routing,
        savings: result.savings,
        injection,
    }
}

pub fn select_canonical(state: &AppState, evaluation: &Evaluation) -> CanonicalRequest {
    let now = langbridge_core::now_ms();
    let created_at = langbridge_core::time::iso_from_ms(now);
    build_canonical_request(CanonicalInput {
        raw_input: &state.text,
        signals: evaluation.signals.clone(),
        routing: evaluation.routing.clone(),
        injection: evaluation.injection.clone(),
        savings: evaluation.savings.clone(),
        final_output: None,
        request_id: format!("req_{}", now),
        created_at,
    })
}

pub fn select_record(_state: &AppState, evaluation: &Evaluation) -> DecisionRecord {
    let now = langbridge_core::now_ms();
    build_decision_record(
        &evaluation.model.model_id,
        evaluation.signals.clone(),
        evaluation.routing.clone(),
        evaluation.savings.clone(),
        evaluation.injection.clone(),
        ProfileRef {
            version: evaluation.profile.as_ref().map(|p| p.version),
            source: evaluation.profile.as_ref().map(|p| p.measurement.method),
            measured_at: evaluation
                .profile
                .as_ref()
                .map(|p| p.measurement.measured_at.clone()),
        },
        langbridge_core::time::iso_from_ms(now),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CalibrationStatus {
    Idle,
    Running,
    Success,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProbeLogStatus {
    Ok,
    Fail,
    Retry,
    Info,
}

impl ProbeLogStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeLogStatus::Ok => "ok",
            ProbeLogStatus::Fail => "fail",
            ProbeLogStatus::Retry => "retry",
            ProbeLogStatus::Info => "info",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProbeLogItem {
    pub id: u32,
    pub status: ProbeLogStatus,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeProgress {
    pub done: usize,
    pub total: usize,
    pub label: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RerunDiff {
    pub deviation: f64,
    pub stable: bool,
}

#[derive(Debug, Clone)]
pub struct CalibrationState {
    pub status: CalibrationStatus,
    pub error: Option<EngineError>,
    pub progress: f64,
    pub rerun: Option<RerunDiff>,
    pub step: Option<ProbeProgress>,
    pub log: Vec<ProbeLogItem>,
    pub hint: Option<String>,
    pub target_model_id: Option<String>,
}

impl PartialEq for CalibrationState {
    fn eq(&self, other: &Self) -> bool {
        self.status == other.status
            && self.progress == other.progress
            && self.rerun == other.rerun
            && self.step == other.step
            && self.log == other.log
            && self.hint == other.hint
            && self.target_model_id == other.target_model_id
            && self.error.as_ref().map(|e| (e.code, e.message.as_str()))
                == other.error.as_ref().map(|e| (e.code, e.message.as_str()))
    }
}

impl Default for CalibrationState {
    fn default() -> Self {
        Self {
            status: CalibrationStatus::Idle,
            error: None,
            progress: 0.0,
            rerun: None,
            step: None,
            log: Vec::new(),
            hint: None,
            target_model_id: None,
        }
    }
}

impl CalibrationState {
    fn running(target: String) -> Self {
        Self {
            status: CalibrationStatus::Running,
            progress: 0.0,
            step: Some(ProbeProgress {
                done: 0,
                total: 0,
                label: PROBE.preparing.to_string(),
            }),
            target_model_id: Some(target),
            ..Self::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TabId {
    Decision,
    Calibrate,
    Evidence,
    Advanced,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct MeasuredEntry {
    pub id: String,
    pub name: String,
    pub ratio: f64,
    pub measured_at: String,
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub model_id: String,
    pub text: String,
    pub streaming: bool,
    pub r_override: Option<f64>,
    pub probe_base_url: String,
    pub user_system_prompt: String,
    pub signals: Option<Signals>,
    pub profile: Option<ModelProfile>,
    pub calibration: CalibrationState,
    pub tab: TabId,
    pub show_raw: bool,
    pub import_errors: Option<Vec<FieldError>>,
    pub storage_warning: Option<String>,
    pub migrated: bool,
    pub measured: Vec<MeasuredEntry>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            model_id: "deepseek-reasoner".to_string(),
            text: String::new(),
            streaming: false,
            r_override: None,
            probe_base_url: String::new(),
            user_system_prompt: String::new(),
            signals: None,
            profile: None,
            calibration: CalibrationState::default(),
            tab: TabId::Decision,
            show_raw: false,
            import_errors: None,
            storage_warning: None,
            migrated: false,
            measured: Vec::new(),
        }
    }
}

#[derive(Default, serde::Serialize, serde::Deserialize)]
struct UiPrefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    r_override: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    probe_base_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text: Option<String>,
}

pub fn truncate_text(text: &str) -> String {
    if text.chars().count() > MAX_PERSISTED_TEXT {
        text.chars().take(MAX_PERSISTED_TEXT).collect()
    } else {
        text.to_string()
    }
}

fn load_prefs() -> UiPrefs {
    let Some(raw) = io::get_item(UI_KEY) else {
        return UiPrefs::default();
    };
    let Ok(mut prefs) = serde_json::from_str::<UiPrefs>(&raw) else {
        return UiPrefs::default();
    };
    if let Some(text) = prefs.text.as_mut() {
        *text = truncate_text(text);
    }
    if prefs.r_override.map(|v| !v.is_finite()).unwrap_or(false) {
        prefs.r_override = None;
    }
    prefs
}

fn profile_key(model_id: &str) -> String {
    format!("{PROFILE_KEY}.{model_id}")
}

fn parse_for(raw: &str, model_id: &str) -> Option<ModelProfile> {
    let parsed: ParseProfileResult = parse_profile_file(raw, langbridge_core::now_ms());
    parsed
        .profile
        .filter(|profile| profile.model.model_id == model_id)
}

pub fn load_profile(model_id: &str) -> Option<ModelProfile> {
    if let Some(raw) = io::get_item(&profile_key(model_id)) {
        if let Some(profile) = parse_for(&raw, model_id) {
            return Some(profile);
        }
    }
    parse_for(&io::get_item(PROFILE_KEY)?, model_id)
}

pub fn save_profile(profile: &ModelProfile) -> bool {
    let raw = serialize_profile(profile);
    let model_id = &profile.model.model_id;
    let saved = io::set_item(&profile_key(model_id), &raw);
    io::set_item(PROFILE_KEY, &raw);
    if profile.measurement.method == langbridge_core::types::ProfileSource::ApiProbe {
        register_measured(profile);
    }
    saved
}

pub fn clear_profile(model_id: &str) {
    io::remove_item(&profile_key(model_id));
    if let Some(raw) = io::get_item(PROFILE_KEY) {
        if parse_for(&raw, model_id).is_some() {
            io::remove_item(PROFILE_KEY);
        }
    }
    let mut list = load_measured();
    let before = list.len();
    list.retain(|entry| entry.id != model_id);
    if list.len() != before {
        save_measured(&list);
    }
}

pub fn load_measured() -> Vec<MeasuredEntry> {
    let Some(raw) = io::get_item(MEASURED_KEY) else {
        return Vec::new();
    };
    let Ok(mut list) = serde_json::from_str::<Vec<MeasuredEntry>>(&raw) else {
        return Vec::new();
    };
    list.retain(|entry| {
        !entry.id.is_empty()
            && entry.ratio.is_finite()
            && entry.ratio > 0.0
            && load_profile(&entry.id)
                .map(|profile| {
                    profile.measurement.method
                        == langbridge_core::types::ProfileSource::ApiProbe
                })
                .unwrap_or(false)
    });
    list.truncate(MEASURED_LIMIT);
    list
}

fn save_measured(list: &[MeasuredEntry]) {
    if let Ok(raw) = serde_json::to_string(list) {
        io::set_item(MEASURED_KEY, &raw);
    }
}

fn register_measured(profile: &ModelProfile) {
    let mut list = load_measured();
    let entry = MeasuredEntry {
        id: profile.model.model_id.clone(),
        name: profile.model.display_name.clone(),
        ratio: profile.measurement.effective_ratio,
        measured_at: profile.measurement.measured_at.clone(),
    };
    list.retain(|item| item.id != entry.id);
    list.insert(0, entry);
    list.truncate(MEASURED_LIMIT);
    save_measured(&list);
}

#[derive(Clone, Copy)]
pub struct AppCtx {
    pub state: RwSignal<AppState>,
    pub evaluation: Memo<Evaluation>,
}

impl AppCtx {
    pub fn evaluation(&self) -> Evaluation {
        self.evaluation.get()
    }

    pub fn set_text(&self, text: String) {
        self.state.update(|s| {
            s.text = text;
            s.import_errors = None;
        });
    }

    pub fn set_model(&self, model_id: String) {
        bump_generation();
        abort_probe();
        let profile = load_profile(&model_id);
        self.state.update(|s| {
            s.model_id = model_id;
            s.profile = profile;
            s.r_override = None;
            s.calibration = CalibrationState {
                status: if s.profile.is_some() {
                    CalibrationStatus::Success
                } else {
                    CalibrationStatus::Idle
                },
                progress: if s.profile.is_some() { 1.0 } else { 0.0 },
                ..CalibrationState::default()
            };
            s.import_errors = None;
            s.migrated = false;
        });
        let text = self.state.get_untracked().text;
        let signals = extract_signals(&text, "");
        self.state.update(|s| s.signals = Some(signals));
    }

    pub fn load_demo(&self) {
        let example = crate::data::EXAMPLES
            .iter()
            .find(|item| item.id == "english")
            .unwrap_or(&crate::data::EXAMPLES[0]);
        self.set_text(example.text.to_string());
        if let Some(id) = crate::data::demo_model_id() {
            if id != self.state.get_untracked().model_id {
                self.set_model(id.clone());
            }
            if load_profile(&id).is_none() {
                self.calibrate(Some(id));
            }
        }
    }

    pub fn load_model_demo(&self, model_id: String) {
        if self.state.get_untracked().text.trim().is_empty() {
            let example = crate::data::EXAMPLES
                .iter()
                .find(|item| item.id == "english")
                .unwrap_or(&crate::data::EXAMPLES[0]);
            self.set_text(example.text.to_string());
        }
        if model_id != self.state.get_untracked().model_id {
            self.set_model(model_id.clone());
        }
        if load_profile(&model_id).is_none()
            && langbridge_core::evidence::has_evidence(&model_id)
        {
            self.calibrate(Some(model_id));
        }
    }

    pub fn set_probe_base_url(&self, value: String) {
        self.state.update(|s| s.probe_base_url = value);
    }

    pub fn set_r_override(&self, r: Option<f64>) {
        self.state.update(|s| s.r_override = r);
    }

    pub fn toggle_streaming(&self) {
        self.state.update(|s| s.streaming = !s.streaming);
    }

    pub fn set_system_prompt(&self, value: String) {
        self.state.update(|s| s.user_system_prompt = value);
    }

    pub fn set_tab(&self, tab: TabId) {
        self.state.update(|s| s.tab = tab);
    }

    pub fn toggle_raw(&self) {
        self.state.update(|s| s.show_raw = !s.show_raw);
    }

    pub fn calibrate(&self, target: Option<String>) {
        let state_signal = self.state;
        let current = state_signal.get_untracked();
        let model_id = target.unwrap_or_else(|| current.model_id.clone());
        bump_generation();
        abort_probe();
        state_signal.update(|s| s.calibration = CalibrationState::running(model_id.clone()));

        let calibration = match calibrate_from_evidence(&model_id) {
            Ok(value) => value,
            Err(error) => {
                state_signal.update(|s| {
                    s.calibration.status = CalibrationStatus::Error;
                    s.calibration.error = Some(error);
                    s.calibration.progress = 0.0;
                    s.calibration.target_model_id = None;
                });
                return;
            }
        };
        let profile = profile_from_calibration(
            &model_id,
            calibration.clone(),
            Some(ProfileSource::Builtin),
        );
        let prev = current
            .profile
            .as_ref()
            .filter(|p| p.model.model_id == model_id)
            .map(|p| p.measurement.clone());
        let rerun = prev.map(|prev| rerun_diff(&prev, &calibration));
        apply_success(state_signal, profile, rerun);
    }

    pub fn calibrate_api(&self, base_url: String, api_key: String, model_id: String) {
        let state_signal = self.state;
        let generation = bump_generation();
        let controller = abort_probe();
        let controller = controller.unwrap_or_else(new_abort);

        state_signal.update(|s| s.calibration = CalibrationState::running(model_id.clone()));
        let log_seq = Rc::new(Cell::new(0u32));
        let target_generation = generation;

        let emit = {
            let log_seq = Rc::clone(&log_seq);
            Rc::new(move |event: ProbeEvent| {
                if current_generation() != target_generation {
                    return;
                }
                if state_signal.get_untracked().calibration.status
                    != CalibrationStatus::Running
                {
                    return;
                }
                match event {
                    ProbeEvent::Start { total } => {
                        state_signal.update(|s| {
                            s.calibration.step = Some(ProbeProgress {
                                done: 0,
                                total,
                                label: PROBE.preparing.to_string(),
                            });
                        });
                    }
                    ProbeEvent::Step { step } => {
                        state_signal.update(|s| {
                            let total = s.calibration.step.as_ref().map(|p| p.total).unwrap_or(0);
                            s.calibration.step = Some(ProbeProgress {
                                done: step.index.saturating_sub(1),
                                total,
                                label: PROBE.step_label(&step),
                            });
                        });
                    }
                    ProbeEvent::Retry {
                        attempt,
                        delay_ms,
                        message,
                        ..
                    } => {
                        let id = next_log(&log_seq);
                        push_log(
                            state_signal,
                            id,
                            ProbeLogStatus::Retry,
                            PROBE.log_retry(attempt, delay_ms, &message),
                        );
                    }
                    ProbeEvent::Ok {
                        step,
                        prompt_tokens,
                    } => {
                        let last = step.index >= step.total;
                        state_signal.update(|s| {
                            s.calibration.step = Some(ProbeProgress {
                                done: step.index,
                                total: step.total,
                                label: if last {
                                    PROBE.finishing.to_string()
                                } else {
                                    PROBE.progress_count(step.index, step.total)
                                },
                            });
                            s.calibration.progress =
                                if step.total > 0 { step.index as f64 / step.total as f64 } else { 0.0 };
                        });
                        let id = next_log(&log_seq);
                        push_log(
                            state_signal,
                            id,
                            ProbeLogStatus::Ok,
                            PROBE.log_ok(lang_text(step.lang), step.n, prompt_tokens),
                        );
                    }
                    ProbeEvent::Fail {
                        step,
                        error,
                        completed,
                        total,
                    } => {
                        let id = next_log(&log_seq);
                        push_log(
                            state_signal,
                            id,
                            ProbeLogStatus::Fail,
                            PROBE.log_fail(lang_text(step.lang), step.n, &error.message),
                        );
                        let _ = (completed, total);
                    }
                }
            }) as Rc<dyn Fn(ProbeEvent)>
        };

        let opts = ProbeOptions {
            base_url,
            api_key,
            model_id: model_id.clone(),
            base_n: None,
            timeout_ms: None,
            retries: None,
            signal: Some(controller.signal()),
            on_event: Some(emit),
        };

        wasm_bindgen_futures::spawn_local(async move {
            let outcome = calibrate_via_api(opts).await;
            if current_generation() != target_generation {
                return;
            }
            let current = state_signal.get_untracked();
            match outcome {
                Ok(calibration) => {
                    let profile = profile_from_calibration(
                        &model_id,
                        calibration.clone(),
                        Some(ProfileSource::ApiProbe),
                    );
                    let prev = current
                        .profile
                        .as_ref()
                        .filter(|p| p.model.model_id == model_id)
                        .map(|p| p.measurement.clone());
                    let rerun = prev.map(|prev| rerun_diff(&prev, &calibration));
                    apply_success(state_signal, profile, rerun);
                }
                Err(error) => {
                    if error.code == langbridge_core::types::ErrorCode::Cancelled {
                        let (done, total) = current
                            .calibration
                            .step
                            .as_ref()
                            .map(|p| (p.done, p.total))
                            .unwrap_or((0, 0));
                        let seq = log_seq.get() + 1;
                        log_seq.set(seq);
                        push_log(
                            state_signal,
                            seq,
                            ProbeLogStatus::Info,
                            PROBE.log_cancelled(done, total),
                        );
                        state_signal.update(|s| {
                            s.calibration.status = CalibrationStatus::Idle;
                            s.calibration.progress = 0.0;
                            s.calibration.step = None;
                            s.calibration.error = None;
                            s.calibration.hint = None;
                            s.calibration.target_model_id = None;
                        });
                        return;
                    }
                    let hint = PROBE
                        .hint(&crate::probe::code_str(error.code))
                        .to_string();
                    state_signal.update(|s| {
                        s.calibration.status = CalibrationStatus::Error;
                        s.calibration.error = Some(error);
                        s.calibration.progress = 0.0;
                        s.calibration.hint = Some(hint);
                        s.calibration.target_model_id = None;
                    });
                }
            }
        });
    }

    pub fn cancel_probe(&self) {
        abort_probe();
        self.state.update(|s| {
            if s.calibration.status == CalibrationStatus::Running {
                s.calibration.status = CalibrationStatus::Idle;
                s.calibration.step = None;
            }
        });
    }

    pub async fn check_connection_async(
        &self,
        base_url: String,
        api_key: String,
        model_id: String,
    ) -> ConnectionCheck {
        test_connection(ProbeOptions {
            base_url,
            api_key,
            model_id,
            base_n: None,
            timeout_ms: Some(8000.0),
            retries: Some(0),
            signal: None,
            on_event: None,
        })
        .await
    }

    pub fn import_profile(&self, text: &str) {
        bump_generation();
        abort_probe();
        let parsed = parse_profile_file(text, langbridge_core::now_ms());
        if !parsed.ok || parsed.profile.is_none() {
            self.state.update(|s| s.import_errors = Some(parsed.errors));
            return;
        }
        let profile = parsed.profile.expect("checked");
        let saved = save_profile(&profile);
        let model_id = profile.model.model_id.clone();
        self.state.update(|s| {
            s.profile = Some(profile);
            s.model_id = model_id;
            s.calibration = CalibrationState {
                status: CalibrationStatus::Success,
                progress: 1.0,
                ..CalibrationState::default()
            };
            s.import_errors = None;
            s.migrated = parsed.migrated;
            s.r_override = None;
            s.storage_warning = if saved {
                None
            } else {
                Some(STORAGE.profile_imported_not_saved.to_string())
            };
            s.tab = TabId::Decision;
            s.measured = load_measured();
        });
    }

    pub fn reset_profile(&self) {
        bump_generation();
        abort_probe();
        let current = self.state.get_untracked();
        let model_id = current
            .profile
            .as_ref()
            .map(|p| p.model.model_id.clone())
            .unwrap_or(current.model_id);
        clear_profile(&model_id);
        self.state.update(|s| {
            s.profile = None;
            s.r_override = None;
            s.storage_warning = None;
            s.calibration = CalibrationState::default();
            s.migrated = false;
            s.measured = load_measured();
        });
    }

    pub fn set_storage_warning(&self, message: Option<String>) {
        self.state.update(|s| s.storage_warning = message);
    }
}

fn rerun_diff(prev: &CalibrationResult, next: &CalibrationResult) -> RerunDiff {
    let (deviation, stable) = compare_rerun(prev, next);
    RerunDiff { deviation, stable }
}

fn apply_success(state: RwSignal<AppState>, profile: ModelProfile, rerun: Option<RerunDiff>) {
    let model_id = profile.model.model_id.clone();
    let target_ok = state
        .get_untracked()
        .calibration
        .target_model_id
        .as_ref()
        .map(|target| target == &model_id)
        .unwrap_or(false);
    if !target_ok {
        return;
    }
    let saved = save_profile(&profile);
    state.update(|s| {
        s.profile = Some(profile);
        s.model_id = model_id;
        s.r_override = None;
        s.storage_warning = if saved {
            None
        } else {
            Some(STORAGE.profile_not_saved.to_string())
        };
        s.tab = if rerun.is_some() {
            TabId::Calibrate
        } else {
            TabId::Decision
        };
        s.calibration = CalibrationState {
            status: CalibrationStatus::Success,
            progress: 1.0,
            rerun,
            log: s.calibration.log.clone(),
            ..CalibrationState::default()
        };
        s.migrated = false;
        s.measured = load_measured();
    });
}

fn push_log(state: RwSignal<AppState>, id: u32, status: ProbeLogStatus, text: String) {
    state.update(|s| {
        s.calibration.log.push(ProbeLogItem { id, status, text });
        let len = s.calibration.log.len();
        if len > 8 {
            s.calibration.log.drain(0..len - 8);
        }
    });
}

fn next_log(seq: &Rc<Cell<u32>>) -> u32 {
    let next = seq.get() + 1;
    seq.set(next);
    next
}

thread_local! {
    static GENERATION: Cell<u32> = const { Cell::new(0) };
    static CONTROLLER: RefCell<Option<web_sys::AbortController>> = const { RefCell::new(None) };
}

fn bump_generation() -> u32 {
    GENERATION.with(|g| {
        let next = g.get().wrapping_add(1);
        g.set(next);
        next
    })
}

fn current_generation() -> u32 {
    GENERATION.with(|g| g.get())
}

fn new_abort() -> web_sys::AbortController {
    web_sys::AbortController::new().expect("AbortController 可用")
}

fn abort_probe() -> Option<web_sys::AbortController> {
    let previous = CONTROLLER.with(|slot| slot.borrow_mut().take());
    if let Some(controller) = previous.as_ref() {
        controller.abort();
    }
    previous
}

pub fn install_controller(controller: web_sys::AbortController) {
    CONTROLLER.with(|slot| *slot.borrow_mut() = Some(controller));
}

pub fn create_initial_state() -> AppState {
    let prefs = load_prefs();
    let model_id = prefs
        .model_id
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| AppState::default().model_id);
    let text = prefs.text.unwrap_or_default();
    let profile = load_profile(&model_id);
    let signals = extract_signals(&text, "");
    AppState {
        model_id,
        text,
        r_override: prefs.r_override,
        probe_base_url: prefs.probe_base_url.unwrap_or_default(),
        profile: profile.clone(),
        signals: Some(signals),
        calibration: CalibrationState {
            status: if profile.is_some() {
                CalibrationStatus::Success
            } else {
                CalibrationStatus::Idle
            },
            progress: if profile.is_some() { 1.0 } else { 0.0 },
            ..CalibrationState::default()
        },
        measured: load_measured(),
        ..AppState::default()
    }
}

fn persist_prefs(state: &AppState) -> Result<(), ()> {
    if !io::storage_available() {
        return Err(());
    }
    let prefs = UiPrefs {
        model_id: Some(state.model_id.clone()),
        r_override: state.r_override,
        probe_base_url: Some(state.probe_base_url.clone()),
        text: Some(truncate_text(&state.text)),
    };
    let Ok(json) = serde_json::to_string(&prefs) else {
        return Err(());
    };
    if io::set_item(UI_KEY, &json) {
        return Ok(());
    }
    let fallback = UiPrefs {
        model_id: prefs.model_id,
        r_override: prefs.r_override,
        probe_base_url: prefs.probe_base_url,
        text: None,
    };
    match serde_json::to_string(&fallback) {
        Ok(json) if io::set_item(UI_KEY, &json) => Ok(()),
        _ => Err(()),
    }
}

pub fn install_app_state() -> AppCtx {
    let state = RwSignal::new(create_initial_state());
    let evaluation = Memo::new(move |_| {
        let snapshot = state.get();
        select_evaluation(&snapshot, langbridge_core::now_ms())
    });

    Effect::new({
        let state = state;
        move |_| {
            let text = state.get().text;
            set_timeout(
                move || {
                    let signals = extract_signals(&text, "");
                    let changed = state
                        .get_untracked()
                        .signals
                        .as_ref()
                        .map(|current| !signals_equal(current, &signals))
                        .unwrap_or(true);
                    if changed {
                        state.update(|s| s.signals = Some(signals));
                    }
                },
                std::time::Duration::from_millis(120),
            );
        }
    });

    let warned = Rc::new(Cell::new(false));
    let warn_timer = Rc::new(RefCell::new(None::<gloo_timers::callback::Timeout>));
    Effect::new({
        let warned = Rc::clone(&warned);
        let warn_timer = Rc::clone(&warn_timer);
        move |_| {
            let snapshot = state.get();
            let _ = snapshot.tab;
            let _ = snapshot.show_raw;
            let _ = snapshot.user_system_prompt;
            let over_limit = snapshot.text.chars().count() > MAX_PERSISTED_TEXT;
            set_timeout(
                {
                    let warned = Rc::clone(&warned);
                    let warn_timer = Rc::clone(&warn_timer);
                    move || {
                        if over_limit && !warned.get() {
                            warned.set(true);
                            state.update(|s| {
                                s.storage_warning =
                                    Some(STORAGE.text_trimmed(MAX_PERSISTED_TEXT))
                            });
                        }
                        if persist_prefs(&state.get_untracked()).is_err() && !warned.get() {
                            warned.set(true);
                            state.update(|s| {
                                s.storage_warning = Some(STORAGE.unavailable.to_string())
                            });
                            *warn_timer.borrow_mut() = Some(gloo_timers::callback::Timeout::new(
                                8000,
                                move || {
                                    state.update(|s| {
                                        if s.storage_warning.as_deref()
                                            == Some(STORAGE.unavailable)
                                        {
                                            s.storage_warning = None;
                                        }
                                    });
                                },
                            ));
                        }
                    }
                },
                std::time::Duration::from_millis(300),
            );
        }
    });

    AppCtx { state, evaluation }
}

fn set_timeout<F: FnOnce() + 'static>(f: F, after: std::time::Duration) {
    gloo_timers::callback::Timeout::new(after.as_millis() as u32, f).forget();
}

fn signals_equal(a: &Signals, b: &Signals) -> bool {
    a.lang == b.lang
        && a.task == b.task
        && a.mixed == b.mixed
        && a.has_code == b.has_code
        && a.has_math == b.has_math
        && a.input_chars == b.input_chars
        && a.input_tokens_est == b.input_tokens_est
        && a.history_tokens_est == b.history_tokens_est
        && a.lang_confidence == b.lang_confidence
        && a.zh_mass == b.zh_mass
        && a.en_mass == b.en_mass
}

pub fn lang_name(value: Lang) -> &'static str {
    lang_text(value)
}

pub fn source_key(source: ProfileSource) -> &'static str {
    match source {
        ProfileSource::Builtin => "builtin",
        ProfileSource::ApiProbe => "api_probe",
        ProfileSource::LiveRun => "live_run",
        ProfileSource::Manual => "manual",
    }
}
