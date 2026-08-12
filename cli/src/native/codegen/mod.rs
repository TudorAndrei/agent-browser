//! Capture agent-browser actions as Chrome DevTools Recorder flows.

mod playwright;
pub mod probe;
mod sidecar;
mod steps;

pub use playwright::render_playwright;
#[allow(unused_imports)]
pub use steps::{
    attach_navigation, enrich_recent_steps, has_frame_scope, mark_popup, record_action,
    set_frame_scope, ClickKind, Scope, Step, Target,
};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodegenStatus {
    #[default]
    Inactive,
    Active,
    Restored,
    Degraded,
    RecoveryError,
    CleanupPending,
}

impl CodegenStatus {
    fn is_capturing(&self) -> bool {
        matches!(self, Self::Active | Self::Restored | Self::Degraded)
    }
}

#[derive(Clone, Debug)]
struct ActionSpan {
    action_id: u64,
    action: String,
    start: usize,
    len: usize,
    journaled: bool,
    persisted_steps: Vec<Step>,
    step_ids: Vec<u64>,
}

/// State intentionally lives with the daemon so a flow spans browser relaunches.
pub struct CodegenState {
    pub status: CodegenStatus,
    pub title: String,
    pub steps: Vec<Step>,
    pub viewport_emitted: bool,
    pub last_url: Option<String>,
    pub start_tab: Option<String>,
    pub sidecar_path: Option<PathBuf>,
    pub known_tabs: HashSet<String>,
    pub capture_errors: Vec<String>,
    pub cleanup_paths: Vec<PathBuf>,
    pub artifact_path: Option<String>,
    action_spans: Vec<ActionSpan>,
    next_action_id: u64,
    next_step_id: u64,
    next_sequence: u64,
    persisted_last_url: Option<String>,
}

impl CodegenState {
    pub fn new() -> Self {
        Self {
            status: CodegenStatus::Inactive,
            title: "agent-browser flow".to_string(),
            steps: Vec::new(),
            viewport_emitted: false,
            last_url: None,
            start_tab: None,
            sidecar_path: None,
            known_tabs: HashSet::new(),
            capture_errors: Vec::new(),
            cleanup_paths: Vec::new(),
            artifact_path: None,
            action_spans: Vec::new(),
            next_action_id: 1,
            next_step_id: 1,
            next_sequence: 1,
            persisted_last_url: None,
        }
    }

    pub fn is_active(&self) -> bool {
        self.status.is_capturing()
    }

    pub fn restore(session_id: &str) -> Self {
        let path = sidecar::path_for_session(session_id);
        let existing = sidecar::existing_known_paths(&path);
        if existing.is_empty() {
            return Self::new();
        }
        if !path.exists() {
            let mut state = Self::new();
            state.status = CodegenStatus::RecoveryError;
            state.sidecar_path = Some(path);
            state.cleanup_paths = existing;
            state.capture_errors.push(
                "Unsupported codegen metadata from an earlier development build. Run `codegen discard`."
                    .to_string(),
            );
            return state;
        }
        match sidecar::recover(&path) {
            Ok(recovered) => Self::from_recovered(path, recovered),
            Err(error) => {
                let mut state = Self::new();
                state.status = CodegenStatus::RecoveryError;
                state.sidecar_path = Some(path.clone());
                state.cleanup_paths = sidecar::existing_known_paths(&path);
                state.capture_errors.push(error);
                state
            }
        }
    }

    fn from_recovered(path: PathBuf, recovered: sidecar::RecoveredJournal) -> Self {
        let mut state = Self::new();
        state.title = recovered.title;
        state.last_url = recovered.last_url.clone();
        state.persisted_last_url = recovered.last_url;
        state.next_sequence = recovered.next_sequence;
        state.sidecar_path = Some(path.clone());
        state.capture_errors = recovered.degraded_messages;
        state.capture_errors.extend(recovered.warnings);
        state.artifact_path = recovered
            .terminal
            .as_ref()
            .and_then(|terminal| match terminal {
                sidecar::TerminalRecord::OutputWritten { path, .. } => path.clone(),
                sidecar::TerminalRecord::DiscardRequested => None,
            });
        state.status = if recovered.terminal.is_some() {
            state.cleanup_paths = sidecar::existing_known_paths(&path);
            CodegenStatus::CleanupPending
        } else if state.capture_errors.is_empty() {
            CodegenStatus::Restored
        } else {
            CodegenStatus::Degraded
        };
        for action in recovered.actions {
            let start = state.steps.len();
            let len = action.steps.len();
            let step_ids = action
                .steps
                .iter()
                .map(|captured| captured.step_id)
                .collect::<Vec<_>>();
            let steps = action
                .steps
                .iter()
                .map(|captured| captured.step.clone())
                .collect::<Vec<_>>();
            state.steps.extend(steps.clone());
            state.next_action_id = state.next_action_id.max(action.action_id + 1);
            state.next_step_id = step_ids
                .iter()
                .fold(state.next_step_id, |next, step_id| next.max(step_id + 1));
            state.action_spans.push(ActionSpan {
                action_id: action.action_id,
                action: action.action,
                start,
                len,
                journaled: true,
                persisted_steps: steps,
                step_ids,
            });
        }
        state.viewport_emitted = state
            .steps
            .iter()
            .any(|step| matches!(step, Step::SetViewport { .. }));
        state
    }

    fn append_record(&mut self, record: sidecar::JournalRecord) -> Result<(), String> {
        let path = self
            .sidecar_path
            .as_ref()
            .ok_or_else(|| "Codegen journal path is not available.".to_string())?;
        sidecar::append(path, self.next_sequence, &record)?;
        self.next_sequence += 1;
        Ok(())
    }

    fn mark_degraded(&mut self, error: String) {
        if !self.capture_errors.contains(&error) {
            self.capture_errors.push(error.clone());
        }
        self.status = CodegenStatus::Degraded;
        let _ = self.append_record(sidecar::JournalRecord::Degraded { message: error });
    }

    pub fn capture_action(&mut self, action: &str, steps: Vec<Step>) {
        if steps.is_empty() {
            return;
        }
        let action_id = self.next_action_id;
        self.next_action_id += 1;
        let start = self.steps.len();
        let len = steps.len();
        let step_ids = (self.next_step_id..self.next_step_id + len as u64).collect::<Vec<_>>();
        self.next_step_id += len as u64;
        self.steps.extend(steps.clone());
        let captured = sidecar::CapturedAction {
            action_id,
            action: action.to_string(),
            steps: step_ids
                .iter()
                .copied()
                .zip(steps.iter().cloned())
                .map(|(step_id, step)| sidecar::CapturedStep { step_id, step })
                .collect(),
        };
        let journaled = match self.append_record(sidecar::JournalRecord::Action(captured)) {
            Ok(()) => true,
            Err(error) => {
                self.mark_degraded(error);
                false
            }
        };
        self.action_spans.push(ActionSpan {
            action_id,
            action: action.to_string(),
            start,
            len,
            journaled,
            persisted_steps: if journaled { steps } else { Vec::new() },
            step_ids,
        });
    }

    fn reset(&mut self) {
        *self = Self::new();
    }
}

pub fn codegen_start(
    state: &mut CodegenState,
    title: Option<&str>,
    session_id: &str,
) -> Result<Value, String> {
    if state.status != CodegenStatus::Inactive {
        return Err(match state.status {
            CodegenStatus::Active | CodegenStatus::Restored | CodegenStatus::Degraded => {
                "Codegen already has a recording. Run `codegen stop` or `codegen discard`."
                    .to_string()
            }
            CodegenStatus::RecoveryError | CodegenStatus::CleanupPending => {
                "Codegen recovery requires cleanup. Run `codegen discard`.".to_string()
            }
            CodegenStatus::Inactive => unreachable!(),
        });
    }
    let title = title
        .filter(|title| !title.is_empty())
        .unwrap_or("agent-browser flow")
        .to_string();
    let expected_path = sidecar::path_for_session(session_id);
    let (path, next_sequence) = match sidecar::create(session_id, &title) {
        Ok(created) => created,
        Err(error) => {
            let existing = sidecar::existing_known_paths(&expected_path);
            if !existing.is_empty() {
                state.status = CodegenStatus::RecoveryError;
                state.sidecar_path = Some(expected_path);
                state.cleanup_paths = existing;
                state.capture_errors.push(error.clone());
            }
            return Err(error);
        }
    };
    state.reset();
    state.status = CodegenStatus::Active;
    state.title = title;
    state.sidecar_path = Some(path);
    state.next_sequence = next_sequence;
    Ok(json!({
        "started": true,
        "state": state.status,
        "active": true,
        "title": state.title,
        "journalPath": state.sidecar_path,
    }))
}

pub fn codegen_status(state: &CodegenState) -> Value {
    json!({
        "state": state.status,
        "active": state.is_active(),
        "title": state.title,
        "steps": state.steps.len(),
        "capturedActions": state.action_spans.len(),
        "warningCount": state.capture_errors.len(),
        "journalPath": state.sidecar_path,
        "captureErrors": state.capture_errors,
        "cleanupPaths": state.cleanup_paths,
        "artifactPath": state.artifact_path,
    })
}

pub fn persist(state: &mut CodegenState) -> Result<(), String> {
    if !state.is_active() {
        return Ok(());
    }
    let mut first_error = None;
    for index in 0..state.action_spans.len() {
        let (journaled, start, len, persisted_steps) = {
            let span = &state.action_spans[index];
            (
                span.journaled,
                span.start,
                span.len,
                span.persisted_steps.clone(),
            )
        };
        if !journaled {
            continue;
        }
        let current_steps = state.steps[start..start + len].to_vec();
        if current_steps == persisted_steps {
            continue;
        }
        let captured = {
            let span = &state.action_spans[index];
            sidecar::CapturedAction {
                action_id: span.action_id,
                action: span.action.clone(),
                steps: span
                    .step_ids
                    .iter()
                    .copied()
                    .zip(current_steps.iter().cloned())
                    .map(|(step_id, step)| sidecar::CapturedStep { step_id, step })
                    .collect(),
            }
        };
        match state.append_record(sidecar::JournalRecord::UpdateAction(captured)) {
            Ok(()) => state.action_spans[index].persisted_steps = current_steps,
            Err(error) => {
                first_error.get_or_insert_with(|| error.clone());
                state.mark_degraded(error);
                break;
            }
        }
    }
    if state.last_url != state.persisted_last_url {
        match state.append_record(sidecar::JournalRecord::State {
            last_url: state.last_url.clone(),
        }) {
            Ok(()) => state.persisted_last_url = state.last_url.clone(),
            Err(error) => {
                first_error.get_or_insert_with(|| error.clone());
                state.mark_degraded(error);
            }
        }
    }
    match first_error {
        Some(error) => Err(error),
        None => Ok(()),
    }
}

pub fn codegen_stop(
    state: &mut CodegenState,
    path: Option<&str>,
    format: &str,
) -> Result<Value, String> {
    if !state.is_active() {
        return Err(match state.status {
            CodegenStatus::CleanupPending => {
                "Codegen output is complete, but journal cleanup is pending. Run `codegen discard`."
                    .to_string()
            }
            CodegenStatus::RecoveryError => {
                "Codegen journal cannot be recovered. Run `codegen discard`.".to_string()
            }
            _ => "No codegen flow in progress. Start one with `codegen start`.".to_string(),
        });
    }
    let steps = if state.status == CodegenStatus::Degraded {
        state.steps.clone()
    } else {
        let journal_path = state
            .sidecar_path
            .as_ref()
            .ok_or_else(|| "Codegen journal path is not available.".to_string())?;
        match sidecar::recover(journal_path) {
            Ok(recovered) => recovered
                .actions
                .into_iter()
                .flat_map(|action| action.steps.into_iter().map(|captured| captured.step))
                .collect(),
            Err(error) => {
                state.status = CodegenStatus::RecoveryError;
                state.capture_errors.push(error.clone());
                return Err(error);
            }
        }
    };
    let flow = json!({ "title": state.title, "steps": steps.iter().map(Step::to_recorder_json).filter(|step| !step.is_null()).collect::<Vec<_>>() });
    let output = if format == "playwright" {
        render_playwright(&state.title, &steps)
    } else {
        serde_json::to_string_pretty(&flow).map_err(|error| error.to_string())?
    };
    if let Some(path) = path {
        sidecar::write_output_atomic(Path::new(path), &output)?;
    }
    if let Err(error) = state.append_record(sidecar::JournalRecord::OutputWritten {
        format: format.to_string(),
        path: path.map(str::to_string),
    }) {
        state.mark_degraded(error.clone());
        return Err(format!(
            "Codegen output was generated, but its terminal journal record failed: {error}. Retry `codegen stop` or run `codegen discard`."
        ));
    }

    let password_steps: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| step.has_password().then_some(index + 1))
        .collect();
    let mut data = json!({
        "state": "inactive",
        "active": false,
        "title": state.title,
        "steps": steps.len(),
        "capturedActions": state.action_spans.len(),
        "format": format,
        "flow": flow,
        "captureErrors": state.capture_errors,
    });
    if let Some(path) = path {
        data["path"] = json!(path);
    } else {
        data["output"] = json!(output);
    }
    if !password_steps.is_empty() {
        data["warning"] = json!(format!(
            "Recorded credentials in step(s) {} verbatim. Do not commit this artifact.",
            password_steps
                .iter()
                .map(usize::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    state.status = CodegenStatus::Inactive;
    state.artifact_path = path.map(str::to_string);
    let journal_path = state
        .sidecar_path
        .clone()
        .ok_or_else(|| "Codegen journal path is not available.".to_string())?;
    match sidecar::remove_known_files(&journal_path) {
        Ok(()) => {
            state.sidecar_path = None;
            state.cleanup_paths.clear();
            Ok(data)
        }
        Err(remaining) => {
            state.status = CodegenStatus::CleanupPending;
            state.cleanup_paths = remaining.clone();
            Err(format!(
                "Codegen output was written{}, but credential-bearing journal cleanup failed for: {}. Run `codegen discard`.",
                path.map(|value| format!(" to {value}"))
                    .unwrap_or_default(),
                remaining
                    .iter()
                    .map(|value| value.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }
}

pub fn codegen_discard(state: &mut CodegenState, session_id: &str) -> Result<Value, String> {
    let journal_path = state
        .sidecar_path
        .clone()
        .unwrap_or_else(|| sidecar::path_for_session(session_id));
    let existing = sidecar::existing_known_paths(&journal_path);
    if state.status == CodegenStatus::Inactive && existing.is_empty() {
        return Err("No codegen recording exists to discard.".to_string());
    }
    if state.is_active() {
        state.append_record(sidecar::JournalRecord::DiscardRequested)?;
    }
    match sidecar::remove_known_files(&journal_path) {
        Ok(()) => {
            let removed = existing
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>();
            state.reset();
            Ok(json!({
                "discarded": true,
                "state": "inactive",
                "active": false,
                "removedPaths": removed,
            }))
        }
        Err(remaining) => {
            state.status = CodegenStatus::CleanupPending;
            state.cleanup_paths = remaining.clone();
            Err(format!(
                "Codegen cleanup is still pending for: {}",
                remaining
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn restored_state(directory: &tempfile::TempDir) -> CodegenState {
        let path = directory.path().join("flow.codegen.jsonl");
        std::fs::write(&path, "").unwrap();
        sidecar::append(
            &path,
            1,
            &sidecar::JournalRecord::Start {
                title: "flow".to_string(),
                last_url: None,
            },
        )
        .unwrap();
        CodegenState::from_recovered(path.clone(), sidecar::recover(&path).unwrap())
    }

    #[test]
    fn restores_an_in_progress_journal() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("flow.codegen.jsonl");
        std::fs::write(&path, "").unwrap();
        sidecar::append(
            &path,
            1,
            &sidecar::JournalRecord::Start {
                title: "checkout".to_string(),
                last_url: Some("https://example.com/cart".to_string()),
            },
        )
        .unwrap();
        sidecar::append(
            &path,
            2,
            &sidecar::JournalRecord::Action(sidecar::CapturedAction {
                action_id: 1,
                action: "viewport".to_string(),
                steps: vec![sidecar::CapturedStep {
                    step_id: 1,
                    step: Step::SetViewport {
                        width: 1280,
                        height: 720,
                        device_scale_factor: 1.0,
                        is_mobile: false,
                    },
                }],
            }),
        )
        .unwrap();

        let state = CodegenState::from_recovered(path.clone(), sidecar::recover(&path).unwrap());

        assert_eq!(state.status, CodegenStatus::Restored);
        assert_eq!(state.title, "checkout");
        assert!(state.viewport_emitted);
        assert_eq!(state.last_url.as_deref(), Some("https://example.com/cart"));
        assert_eq!(state.sidecar_path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn stop_emits_navigation_and_password_warning() {
        let directory = tempfile::tempdir().unwrap();
        let target = Target {
            selectors: vec![vec!["#password".to_string()]],
            role: None,
            name: None,
            nth: None,
            test_id: None,
            input_type: Some("password".to_string()),
        };
        let mut state = restored_state(&directory);
        state.title = "login".to_string();
        state.capture_action(
            "login",
            vec![
                Step::Click {
                    target: target.clone(),
                    count: 1,
                    kind: ClickKind::Click,
                    opens_popup: false,
                    scope: Scope::default(),
                    asserted_url: Some("https://example.com/account".to_string()),
                },
                Step::Change {
                    target,
                    value: "secret".to_string(),
                    is_select: false,
                    scope: Scope::default(),
                    asserted_url: None,
                },
            ],
        );

        let result = codegen_stop(&mut state, None, "json").unwrap();

        assert!(result["warning"].as_str().unwrap().contains("credentials"));
        assert!(result["output"].as_str().unwrap().contains("account"));
        assert!(!directory.path().join("flow.codegen.jsonl").exists());
    }

    #[test]
    fn output_failure_keeps_the_recording_retryable() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = restored_state(&directory);
        state.capture_action(
            "viewport",
            vec![Step::SetViewport {
                width: 800,
                height: 600,
                device_scale_factor: 1.0,
                is_mobile: false,
            }],
        );
        let missing_parent = directory.path().join("missing/flow.json");

        assert!(codegen_stop(&mut state, missing_parent.to_str(), "json").is_err());
        assert!(state.is_active());
        let output = directory.path().join("flow.json");
        assert!(codegen_stop(&mut state, output.to_str(), "json").is_ok());
        assert!(output.exists());
    }

    #[test]
    fn cleanup_failure_after_output_enters_cleanup_pending() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = restored_state(&directory);
        state.capture_action(
            "viewport",
            vec![Step::SetViewport {
                width: 800,
                height: 600,
                device_scale_factor: 1.0,
                is_mobile: false,
            }],
        );
        let journal = state.sidecar_path.clone().unwrap();
        let blocker = sidecar::legacy_metadata_paths(&journal)[0].clone();
        std::fs::create_dir(&blocker).unwrap();
        let output = directory.path().join("flow.json");

        let error = codegen_stop(&mut state, output.to_str(), "json").unwrap_err();

        assert!(error.contains("cleanup failed"));
        assert_eq!(state.status, CodegenStatus::CleanupPending);
        assert!(output.exists());
        std::fs::remove_dir(&blocker).unwrap();
        codegen_discard(&mut state, "unused").unwrap();
    }

    #[test]
    fn discard_retries_cleanup_after_a_terminal_record() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = restored_state(&directory);
        let journal = state.sidecar_path.clone().unwrap();
        let blocker = sidecar::legacy_metadata_paths(&journal)[0].clone();
        std::fs::create_dir(&blocker).unwrap();

        assert!(codegen_discard(&mut state, "unused").is_err());
        assert_eq!(state.status, CodegenStatus::CleanupPending);
        std::fs::remove_dir(&blocker).unwrap();
        let retry = codegen_discard(&mut state, "unused").unwrap();

        assert_eq!(retry["discarded"], true);
        assert_eq!(state.status, CodegenStatus::Inactive);
    }

    #[test]
    fn append_failure_latches_a_degraded_state() {
        let directory = tempfile::tempdir().unwrap();
        let mut state = restored_state(&directory);
        std::fs::remove_file(state.sidecar_path.as_ref().unwrap()).unwrap();

        state.capture_action(
            "viewport",
            vec![Step::SetViewport {
                width: 800,
                height: 600,
                device_scale_factor: 1.0,
                is_mobile: false,
            }],
        );

        assert_eq!(state.status, CodegenStatus::Degraded);
        assert_eq!(state.capture_errors.len(), 1);
    }

    #[test]
    fn status_exposes_recovery_state() {
        let mut state = CodegenState::new();
        state.status = CodegenStatus::RecoveryError;
        state.capture_errors.push("bad journal".to_string());

        let status = codegen_status(&state);

        assert_eq!(status["state"], "recovery-error");
        assert_eq!(status["active"], false);
        assert_eq!(status["warningCount"], 1);
    }

    #[test]
    fn status_reports_all_states_and_derived_activity() {
        for (state_value, name, active) in [
            (CodegenStatus::Inactive, "inactive", false),
            (CodegenStatus::Active, "active", true),
            (CodegenStatus::Restored, "restored", true),
            (CodegenStatus::Degraded, "degraded", true),
            (CodegenStatus::RecoveryError, "recovery-error", false),
            (CodegenStatus::CleanupPending, "cleanup-pending", false),
        ] {
            let mut state = CodegenState::new();
            state.status = state_value;
            let status = codegen_status(&state);
            assert_eq!(status["state"], name);
            assert_eq!(status["active"], active);
        }
    }
}
