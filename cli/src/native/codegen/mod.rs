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

use serde_json::{json, Value};
use std::collections::HashSet;
use std::path::PathBuf;

/// State intentionally lives with the daemon so a flow spans browser relaunches.
pub struct CodegenState {
    pub active: bool,
    pub title: String,
    pub steps: Vec<Step>,
    pub viewport_emitted: bool,
    pub last_url: Option<String>,
    pub start_tab: Option<String>,
    pub sidecar_path: Option<PathBuf>,
    pub known_tabs: HashSet<String>,
}

impl CodegenState {
    pub fn new() -> Self {
        Self {
            active: false,
            title: "agent-browser flow".to_string(),
            steps: Vec::new(),
            viewport_emitted: false,
            last_url: None,
            start_tab: None,
            sidecar_path: None,
            known_tabs: HashSet::new(),
        }
    }

    pub fn restore(session_id: &str) -> Self {
        let Ok(Some((path, metadata))) = sidecar::read_metadata(session_id) else {
            return Self::new();
        };
        Self::restore_from_sidecar(path, metadata)
    }

    fn restore_from_sidecar(path: PathBuf, metadata: sidecar::Metadata) -> Self {
        let mut state = Self::new();
        let Ok(steps) = sidecar::read(&path) else {
            return state;
        };
        state.active = true;
        state.title = metadata.title;
        state.viewport_emitted = steps
            .iter()
            .any(|step| matches!(step, Step::SetViewport { .. }));
        state.last_url = metadata.last_url;
        state.steps = steps;
        state.sidecar_path = Some(path);
        state
    }
}

pub fn codegen_start(
    state: &mut CodegenState,
    title: Option<&str>,
    session_id: &str,
) -> Result<Value, String> {
    if state.active {
        return Err("Codegen already active".to_string());
    }
    state.active = true;
    state.title = title
        .filter(|title| !title.is_empty())
        .unwrap_or("agent-browser flow")
        .to_string();
    state.steps.clear();
    state.viewport_emitted = false;
    state.last_url = None;
    state.start_tab = None;
    state.sidecar_path = Some(sidecar::create(session_id)?);
    state.known_tabs.clear();
    persist(state)?;
    Ok(json!({ "started": true, "title": state.title, "sidecarPath": state.sidecar_path }))
}

pub fn codegen_status(state: &CodegenState) -> Value {
    json!({
        "active": state.active,
        "title": state.title,
        "steps": state.steps.len(),
        "sidecarPath": state.sidecar_path,
    })
}

pub fn persist(state: &CodegenState) -> Result<(), String> {
    if let Some(path) = state.sidecar_path.as_ref() {
        sidecar::rewrite(path, &state.steps)?;
        sidecar::write_metadata(
            path,
            &sidecar::Metadata {
                title: state.title.clone(),
                last_url: state.last_url.clone(),
            },
        )?;
    }
    Ok(())
}

pub fn codegen_stop(
    state: &mut CodegenState,
    path: Option<&str>,
    format: &str,
) -> Result<Value, String> {
    if !state.active {
        return Err("No codegen flow in progress. Start one with `codegen start`.".to_string());
    }
    let steps = match state.sidecar_path.as_ref() {
        Some(sidecar_path) => sidecar::read(sidecar_path).unwrap_or_else(|_| state.steps.clone()),
        None => state.steps.clone(),
    };
    let flow = json!({ "title": state.title, "steps": steps.iter().map(Step::to_recorder_json).filter(|step| !step.is_null()).collect::<Vec<_>>() });
    let output = if format == "playwright" {
        render_playwright(&state.title, &steps)
    } else {
        serde_json::to_string_pretty(&flow).map_err(|e| e.to_string())?
    };
    if let Some(path) = path {
        std::fs::write(path, &output)
            .map_err(|e| format!("Failed to write codegen output: {e}"))?;
    }
    let password_steps: Vec<usize> = steps
        .iter()
        .enumerate()
        .filter_map(|(index, step)| step.has_password().then_some(index + 1))
        .collect();
    state.active = false;
    let mut data =
        json!({ "title": state.title, "steps": steps.len(), "format": format, "flow": flow });
    if let Some(path) = path {
        data["path"] = json!(path);
    }
    if path.is_none() {
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
    if let Some(sidecar_path) = state.sidecar_path.as_ref() {
        sidecar::remove_metadata(sidecar_path);
    }
    Ok(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restores_an_in_progress_sidecar() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flow.codegen.jsonl");
        let steps = vec![Step::SetViewport {
            width: 1280,
            height: 720,
            device_scale_factor: 1.0,
            is_mobile: false,
        }];
        sidecar::rewrite(&path, &steps).unwrap();

        let state = CodegenState::restore_from_sidecar(
            path.clone(),
            sidecar::Metadata {
                title: "checkout".to_string(),
                last_url: Some("https://example.com/cart".to_string()),
            },
        );

        assert!(state.active);
        assert_eq!(state.title, "checkout");
        assert!(state.viewport_emitted);
        assert_eq!(state.last_url.as_deref(), Some("https://example.com/cart"));
        assert_eq!(state.sidecar_path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn stop_emits_navigation_and_password_warning() {
        let target = Target {
            selectors: vec![vec!["#password".to_string()]],
            role: None,
            name: None,
            nth: None,
            test_id: None,
            input_type: Some("password".to_string()),
        };
        let mut state = CodegenState::new();
        state.active = true;
        state.title = "login".to_string();
        state.steps = vec![
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
        ];

        let result = codegen_stop(&mut state, None, "json").unwrap();

        assert_eq!(
            result["flow"]["steps"][0]["assertedEvents"][0]["url"],
            "https://example.com/account"
        );
        assert!(result["flow"]["steps"][1].get("assertedEvents").is_none());
        assert!(result["warning"].as_str().unwrap().contains("step(s) 2"));
        assert!(!state.active);
    }

    #[test]
    fn stop_does_not_warn_for_non_password_change() {
        let target = Target {
            selectors: vec![vec!["#email".to_string()]],
            role: None,
            name: None,
            nth: None,
            test_id: None,
            input_type: Some("email".to_string()),
        };
        let mut state = CodegenState::new();
        state.active = true;
        state.steps.push(Step::Change {
            target,
            value: "person@example.com".to_string(),
            is_select: false,
            scope: Scope::default(),
            asserted_url: None,
        });

        let result = codegen_stop(&mut state, None, "json").unwrap();

        assert!(result.get("warning").is_none());
    }
}
