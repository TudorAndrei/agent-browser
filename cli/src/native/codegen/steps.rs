use super::probe::{self, Probe};
use super::sidecar;
use crate::native::cdp::client::CdpClient;
use crate::native::element::{parse_ref, RefMap};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Target {
    pub selectors: Vec<Vec<String>>,
    pub role: Option<String>,
    pub name: Option<String>,
    pub nth: Option<usize>,
    pub test_id: Option<String>,
    pub input_type: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Scope {
    pub target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame: Vec<usize>,
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            target: "main".to_string(),
            frame: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ClickKind {
    Click,
    Check,
    Uncheck,
    Tap,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Step {
    SetViewport {
        width: i64,
        height: i64,
        device_scale_factor: f64,
        is_mobile: bool,
    },
    Navigate {
        url: String,
    },
    Click {
        target: Target,
        count: u8,
        kind: ClickKind,
        opens_popup: bool,
        scope: Scope,
        asserted_url: Option<String>,
    },
    Hover {
        target: Target,
        scope: Scope,
    },
    Change {
        target: Target,
        value: String,
        is_select: bool,
        scope: Scope,
        asserted_url: Option<String>,
    },
    KeyDown {
        key: String,
        scope: Scope,
    },
    KeyUp {
        key: String,
        scope: Scope,
    },
    Scroll {
        target: Option<Target>,
        x: i64,
        y: i64,
        scope: Scope,
    },
    WaitForElement {
        target: Target,
        scope: Scope,
        visible: Option<bool>,
        properties: Map<String, Value>,
        count: Option<i64>,
        operator: Option<String>,
    },
    Close,
    NewTab {
        url: Option<String>,
    },
}

impl Step {
    pub fn has_password(&self) -> bool {
        matches!(self, Self::Change { target, .. } if target.input_type.as_deref() == Some("password"))
    }

    pub fn to_recorder_json(&self) -> Value {
        match self {
            Self::SetViewport {
                width,
                height,
                device_scale_factor,
                is_mobile,
            } => {
                json!({ "type": "setViewport", "width": width, "height": height, "deviceScaleFactor": device_scale_factor, "isMobile": is_mobile, "hasTouch": is_mobile, "isLandscape": false })
            }
            Self::Navigate { url } => json!({ "type": "navigate", "url": url }),
            Self::Click {
                target,
                count,
                asserted_url,
                scope,
                opens_popup,
                ..
            } => with_scope(
                // Recorder applies an asserted navigation to the clicked page.
                // A popup navigates a different target, which the following scoped
                // step resolves by URL, so keep this assertion for Playwright only.
                if *opens_popup {
                    json!({ "type": "click", "selectors": target.selectors, "offsetX": 0, "offsetY": 0, "button": "primary", "clickCount": count })
                } else {
                    with_navigation(
                        json!({ "type": "click", "selectors": target.selectors, "offsetX": 0, "offsetY": 0, "button": "primary", "clickCount": count }),
                        asserted_url,
                    )
                },
                scope,
            ),
            Self::Hover { target, scope } => with_scope(
                json!({ "type": "hover", "selectors": target.selectors, "offsetX": 0, "offsetY": 0 }),
                scope,
            ),
            Self::Change {
                target,
                value,
                asserted_url,
                scope,
                ..
            } => with_scope(
                with_navigation(
                    json!({ "type": "change", "selectors": target.selectors, "value": value }),
                    asserted_url,
                ),
                scope,
            ),
            Self::KeyDown { key, scope } => {
                with_scope(json!({ "type": "keyDown", "key": key }), scope)
            }
            Self::KeyUp { key, scope } => with_scope(json!({ "type": "keyUp", "key": key }), scope),
            Self::Scroll {
                target,
                x,
                y,
                scope,
            } => {
                let mut value = json!({ "type": "scroll", "x": x, "y": y });
                if let Some(target) = target {
                    value["selectors"] = json!(target.selectors);
                }
                with_scope(value, scope)
            }
            Self::WaitForElement {
                target,
                visible,
                properties,
                count,
                operator,
                scope,
                ..
            } => {
                let mut value = json!({ "type": "waitForElement", "selectors": target.selectors });
                if let Some(visible) = visible {
                    value["visible"] = json!(visible);
                }
                if !properties.is_empty() {
                    value["properties"] = json!(properties);
                }
                if let Some(count) = count {
                    value["count"] = json!(count);
                }
                if let Some(operator) = operator {
                    value["operator"] = json!(operator);
                }
                with_scope(value, scope)
            }
            Self::Close => json!({ "type": "close" }),
            // Recorder has no tab creation primitive. The sidecar retains it so
            // Playwright can faithfully create a page, while JSON stays schema-clean.
            Self::NewTab { .. } => Value::Null,
        }
    }
}

fn with_navigation(mut step: Value, url: &Option<String>) -> Value {
    if let Some(url) = url {
        step["assertedEvents"] = json!([{ "type": "navigation", "url": url }]);
    }
    step
}

fn with_scope(mut step: Value, scope: &Scope) -> Value {
    if scope.target != "main" {
        step["target"] = json!(scope.target);
    }
    if !scope.frame.is_empty() {
        step["frame"] = json!(scope.frame);
    }
    step
}

fn selector_target(selector: &str, refs: &RefMap) -> Target {
    if let Some(reference) = parse_ref(selector).and_then(|reference| refs.get(&reference)) {
        let aria = if reference.name.is_empty() {
            None
        } else {
            Some(format!("aria/{}", reference.name))
        };
        let css = reference.selector.clone();
        let selectors = aria
            .into_iter()
            .chain(css)
            .map(|value| vec![value])
            .collect();
        return Target {
            selectors,
            role: (!reference.role.is_empty()).then(|| reference.role.clone()),
            name: (!reference.name.is_empty()).then(|| reference.name.clone()),
            nth: reference.nth,
            test_id: None,
            input_type: None,
        };
    }
    let value = if let Some(text) = selector.strip_prefix("text=") {
        format!("text/{text}")
    } else if let Some(xpath) = selector.strip_prefix("xpath=") {
        format!("xpath/{xpath}")
    } else if selector.starts_with("//") {
        format!("xpath/{selector}")
    } else {
        selector.to_string()
    };
    Target {
        selectors: vec![vec![value]],
        role: None,
        name: None,
        nth: None,
        test_id: None,
        input_type: None,
    }
}

fn target(cmd: &Value, refs: &RefMap) -> Option<Target> {
    cmd.get("selector")
        .and_then(Value::as_str)
        .map(|selector| selector_target(selector, refs))
}
pub fn record_action(
    action: &str,
    cmd: &Value,
    data: &Value,
    refs: &RefMap,
    viewport: Option<(i32, i32, f64, bool)>,
    scope: Scope,
    state: &mut super::CodegenState,
) -> Result<(), String> {
    let mut steps = Vec::new();
    let sc = scope;
    match action {
        "navigate" => {
            if let Some(url) = cmd.get("url").and_then(Value::as_str) {
                if state.last_url.as_deref() != Some(url) {
                    steps.push(Step::Navigate {
                        url: url.to_string(),
                    });
                    state.last_url = Some(url.to_string());
                }
            }
        }
        "back" | "forward" | "reload" => {
            if let Some(url) = data.get("url").and_then(Value::as_str) {
                steps.push(Step::Navigate {
                    url: url.to_string(),
                });
                state.last_url = Some(url.to_string());
            }
        }
        "click" | "tap" | "dblclick" | "check" | "uncheck" => {
            if let Some(target) = target(cmd, refs) {
                let kind = match action {
                    "check" => ClickKind::Check,
                    "uncheck" => ClickKind::Uncheck,
                    "tap" => ClickKind::Tap,
                    _ => ClickKind::Click,
                };
                steps.push(Step::Click {
                    target,
                    count: if action == "dblclick" { 2 } else { 1 },
                    kind,
                    opens_popup: cmd.get("newTab").and_then(Value::as_bool).unwrap_or(false),
                    scope: sc,
                    asserted_url: None,
                });
            }
        }
        "hover" => {
            if let Some(target) = target(cmd, refs) {
                steps.push(Step::Hover { target, scope: sc });
            }
        }
        "fill" | "setvalue" | "type" | "select" => {
            if let Some(target) = target(cmd, refs) {
                let value = cmd
                    .get(if action == "type" {
                        "text"
                    } else if action == "select" {
                        "values"
                    } else {
                        "value"
                    })
                    .map(|v| {
                        if let Some(s) = v.as_str() {
                            s.to_string()
                        } else {
                            v.to_string()
                        }
                    })
                    .unwrap_or_default();
                steps.push(Step::Change {
                    target,
                    value,
                    is_select: action == "select",
                    scope: sc,
                    asserted_url: None,
                });
            }
        }
        "press" => {
            if let Some(key) = cmd.get("key").and_then(Value::as_str) {
                steps.push(Step::KeyDown {
                    key: key.to_string(),
                    scope: sc.clone(),
                });
                steps.push(Step::KeyUp {
                    key: key.to_string(),
                    scope: sc,
                });
            }
        }
        "scroll" => steps.push(Step::Scroll {
            target: target(cmd, refs),
            x: cmd.get("x").and_then(Value::as_i64).unwrap_or(0),
            y: cmd.get("y").and_then(Value::as_i64).unwrap_or(0),
            scope: sc,
        }),
        "viewport" => {
            if let (Some(width), Some(height)) = (
                cmd.get("width").and_then(Value::as_i64),
                cmd.get("height").and_then(Value::as_i64),
            ) {
                steps.push(Step::SetViewport {
                    width,
                    height,
                    device_scale_factor: cmd
                        .get("deviceScaleFactor")
                        .and_then(Value::as_f64)
                        .unwrap_or(1.0),
                    is_mobile: cmd.get("mobile").and_then(Value::as_bool).unwrap_or(false),
                });
            }
        }
        "isvisible" | "isenabled" | "ischecked" | "count" | "wait" => {
            if let Some(target) = target(cmd, refs) {
                let mut properties = Map::new();
                let mut visible = None;
                let mut count = None;
                if action == "isvisible" {
                    visible = data.get("visible").and_then(Value::as_bool);
                }
                if action == "isenabled" {
                    if let Some(value) = data.get("enabled").and_then(Value::as_bool) {
                        properties.insert("disabled".to_string(), json!(!value));
                    }
                }
                if action == "ischecked" {
                    if let Some(value) = data.get("checked").and_then(Value::as_bool) {
                        properties.insert("checked".to_string(), json!(value));
                    }
                }
                if action == "count" {
                    count = data.get("count").and_then(Value::as_i64);
                }
                if action == "wait" {
                    visible = Some(true);
                }
                steps.push(Step::WaitForElement {
                    target,
                    scope: sc,
                    visible,
                    properties,
                    count,
                    operator: count.map(|_| "==".to_string()),
                });
            }
        }
        "close" => steps.push(Step::Close),
        "tab_new" => steps.push(Step::NewTab {
            url: cmd.get("url").and_then(Value::as_str).map(str::to_string),
        }),
        _ => {}
    }
    if !steps.is_empty() && !state.viewport_emitted {
        let (width, height, scale, mobile) = viewport.unwrap_or((1280, 720, 1.0, false));
        steps.insert(
            0,
            Step::SetViewport {
                width: width.into(),
                height: height.into(),
                device_scale_factor: scale,
                is_mobile: mobile,
            },
        );
        state.viewport_emitted = true;
    }
    for step in steps {
        state.steps.push(step.clone());
        if let Some(path) = state.sidecar_path.as_ref() {
            sidecar::append(path, &step)?;
        }
    }
    Ok(())
}

fn enrich_target(target: &mut Target, probe: &Probe) {
    if let Some(selector) = &probe.selector {
        if !target
            .selectors
            .iter()
            .any(|candidate| candidate == &vec![selector.clone()])
        {
            target.selectors.push(vec![selector.clone()]);
        }
    }
    target.test_id = probe.test_id.clone();
    target.input_type = probe.input_type.clone();
}

fn enrich_step(step: &mut Step, probe: &Probe) {
    match step {
        Step::Click { target, .. }
        | Step::Hover { target, .. }
        | Step::Change { target, .. }
        | Step::WaitForElement { target, .. } => enrich_target(target, probe),
        Step::Scroll {
            target: Some(target),
            ..
        } => enrich_target(target, probe),
        _ => {}
    }
}

/// Resolve a captured `@eN` while its snapshot ref still exists. The probe is
/// deliberately best-effort: ARIA selectors remain useful if CDP resolution fails.
pub async fn enrich_recent_steps(
    steps: &mut [Step],
    selector: Option<&str>,
    refs: &RefMap,
    client: &CdpClient,
    session_id: &str,
) -> Option<String> {
    let entry = selector
        .and_then(parse_ref)
        .and_then(|reference| refs.get(&reference))?;
    let backend_node_id = entry.backend_node_id?;
    let probe = probe::probe_element(client, session_id, backend_node_id)
        .await
        .ok()?;
    for step in steps {
        enrich_step(step, &probe);
    }
    probe.href
}

pub fn attach_navigation(step: &mut Step, url: &str) {
    match step {
        Step::Click { asserted_url, .. } | Step::Change { asserted_url, .. } => {
            *asserted_url = Some(url.to_string())
        }
        _ => {}
    }
}

pub fn mark_popup(step: &mut Step) {
    if let Step::Click { opens_popup, .. } = step {
        *opens_popup = true;
    }
}

pub fn set_frame_scope(steps: &mut [Step], frame: Vec<usize>) {
    for step in steps {
        match step {
            Step::Click { scope, .. }
            | Step::Hover { scope, .. }
            | Step::Change { scope, .. }
            | Step::KeyDown { scope, .. }
            | Step::KeyUp { scope, .. }
            | Step::Scroll { scope, .. }
            | Step::WaitForElement { scope, .. } => scope.frame = frame.clone(),
            _ => {}
        }
    }
}

pub fn has_frame_scope(steps: &[Step]) -> bool {
    steps.iter().any(|step| match step {
        Step::Click { scope, .. }
        | Step::Hover { scope, .. }
        | Step::Change { scope, .. }
        | Step::KeyDown { scope, .. }
        | Step::KeyUp { scope, .. }
        | Step::Scroll { scope, .. }
        | Step::WaitForElement { scope, .. } => !scope.frame.is_empty(),
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::codegen::CodegenState;

    #[test]
    fn captures_core_steps_and_skips_observations() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = CodegenState::new();
        state.active = true;
        state.sidecar_path = Some(dir.path().join("flow.jsonl"));
        std::fs::write(state.sidecar_path.as_ref().unwrap(), "").unwrap();
        let refs = RefMap::new();
        record_action(
            "navigate",
            &json!({ "url": "https://example.com" }),
            &json!({}),
            &refs,
            Some((1280, 720, 1.0, false)),
            Scope::default(),
            &mut state,
        )
        .unwrap();
        record_action(
            "click",
            &json!({ "selector": "#submit" }),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            &mut state,
        )
        .unwrap();
        record_action(
            "isvisible",
            &json!({ "selector": "#success" }),
            &json!({ "visible": false }),
            &refs,
            None,
            Scope::default(),
            &mut state,
        )
        .unwrap();
        record_action(
            "snapshot",
            &json!({}),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            &mut state,
        )
        .unwrap();
        let steps = state
            .steps
            .iter()
            .map(Step::to_recorder_json)
            .collect::<Vec<_>>();
        assert_eq!(steps.len(), 4);
        assert_eq!(steps[0]["type"], "setViewport");
        assert_eq!(steps[1]["type"], "navigate");
        assert_eq!(steps[2]["type"], "click");
        assert_eq!(steps[3]["type"], "waitForElement");
        assert!(matches!(
            state.steps.last(),
            Some(Step::WaitForElement { .. })
        ));
        assert_eq!(state.steps.len(), 4);
    }

    #[test]
    fn records_observed_visibility_and_dedupes_navigation() {
        let mut state = CodegenState::new();
        state.active = true;
        let refs = RefMap::new();
        record_action(
            "navigate",
            &json!({ "url": "https://example.com" }),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            &mut state,
        )
        .unwrap();
        record_action(
            "navigate",
            &json!({ "url": "https://example.com" }),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            &mut state,
        )
        .unwrap();
        record_action(
            "isvisible",
            &json!({ "selector": "#missing" }),
            &json!({ "visible": false }),
            &refs,
            None,
            Scope::default(),
            &mut state,
        )
        .unwrap();
        assert_eq!(state.steps.len(), 3);
        assert_eq!(state.steps[2].to_recorder_json()["visible"], false);
    }

    #[test]
    fn observation_before_first_action_emits_no_viewport() {
        let mut state = CodegenState::new();
        state.active = true;
        record_action(
            "snapshot",
            &json!({}),
            &json!({}),
            &RefMap::new(),
            Some((1280, 720, 1.0, false)),
            Scope::default(),
            &mut state,
        )
        .unwrap();
        assert!(state.steps.is_empty());
        assert!(!state.viewport_emitted);
    }

    #[test]
    fn captures_supported_actions_and_selector_forms() {
        let mut state = CodegenState::new();
        state.active = true;
        let mut refs = RefMap::new();
        refs.add_selector("e1".into(), "#usable".into(), "button", "", None);
        let scope = Scope::default();

        for (action, command, data) in [
            (
                "back",
                json!({}),
                json!({ "url": "https://example.com/back" }),
            ),
            (
                "forward",
                json!({}),
                json!({ "url": "https://example.com/forward" }),
            ),
            (
                "reload",
                json!({}),
                json!({ "url": "https://example.com/reload" }),
            ),
            ("click", json!({ "selector": "@e1" }), json!({})),
            ("tap", json!({ "selector": "text=Tap" }), json!({})),
            (
                "dblclick",
                json!({ "selector": "xpath=//button" }),
                json!({}),
            ),
            ("hover", json!({ "selector": "//a" }), json!({})),
            (
                "fill",
                json!({ "selector": "#field", "value": "value" }),
                json!({}),
            ),
            (
                "setvalue",
                json!({ "selector": "#field", "value": "value" }),
                json!({}),
            ),
            (
                "type",
                json!({ "selector": "#field", "text": "value" }),
                json!({}),
            ),
            (
                "select",
                json!({ "selector": "#select", "values": ["one"] }),
                json!({}),
            ),
            ("check", json!({ "selector": "#check" }), json!({})),
            ("uncheck", json!({ "selector": "#check" }), json!({})),
            ("press", json!({ "key": "Enter" }), json!({})),
            (
                "scroll",
                json!({ "selector": "#scroll", "x": 3, "y": 4 }),
                json!({}),
            ),
            (
                "viewport",
                json!({ "width": 800, "height": 600 }),
                json!({}),
            ),
            ("wait", json!({ "selector": "#ready" }), json!({})),
            (
                "isenabled",
                json!({ "selector": "#ready" }),
                json!({ "enabled": false }),
            ),
            (
                "ischecked",
                json!({ "selector": "#ready" }),
                json!({ "checked": false }),
            ),
            (
                "count",
                json!({ "selector": "#ready" }),
                json!({ "count": 2 }),
            ),
            (
                "tab_new",
                json!({ "url": "https://other.example" }),
                json!({}),
            ),
            ("close", json!({}), json!({})),
        ] {
            record_action(
                action,
                &command,
                &data,
                &refs,
                None,
                scope.clone(),
                &mut state,
            )
            .unwrap();
        }

        let recorder_steps = state
            .steps
            .iter()
            .map(Step::to_recorder_json)
            .filter(|step| !step.is_null())
            .collect::<Vec<_>>();
        let types = recorder_steps
            .iter()
            .filter_map(|step| step.get("type").and_then(Value::as_str))
            .collect::<Vec<_>>();
        assert!(types.contains(&"navigate"));
        assert_eq!(recorder_steps[4]["selectors"][0][0], "#usable");
        assert_eq!(recorder_steps[5]["selectors"][0][0], "text/Tap");
        assert_eq!(recorder_steps[6]["selectors"][0][0], "xpath///button");
        assert_eq!(recorder_steps[7]["selectors"][0][0], "xpath///a");
        assert!(types.contains(&"hover"));
        assert!(types.contains(&"change"));
        assert!(types.contains(&"keyDown"));
        assert!(types.contains(&"keyUp"));
        assert!(types.contains(&"scroll"));
        assert!(types.contains(&"waitForElement"));
        assert!(types.contains(&"close"));
        assert_eq!(
            state
                .steps
                .iter()
                .filter(|step| matches!(step, Step::SetViewport { .. }))
                .count(),
            2,
            "one initial viewport plus the explicit viewport action"
        );
        assert!(matches!(
            state.steps.iter().find(|step| matches!(step, Step::Click { target, .. } if target.selectors == vec![vec!["#usable".to_string()]])),
            Some(_)
        ));
    }

    #[test]
    fn ignores_observations_without_a_recorder_mapping() {
        let mut state = CodegenState::new();
        state.active = true;
        for action in [
            "snapshot",
            "screenshot",
            "gettext",
            "getattribute",
            "console",
            "evaluate",
        ] {
            record_action(
                action,
                &json!({ "selector": "#ignored" }),
                &json!({}),
                &RefMap::new(),
                None,
                Scope::default(),
                &mut state,
            )
            .unwrap();
        }
        assert!(state.steps.is_empty());
    }

    #[test]
    fn main_frame_scope_omits_frame_and_other_tabs_keep_their_url() {
        let main = serde_json::to_value(Scope::default()).unwrap();
        assert_eq!(main["target"], "main");
        assert!(main.get("frame").is_none());

        let tab = Scope {
            target: "https://other.example".to_string(),
            frame: vec![0],
        };
        let tab = serde_json::to_value(tab).unwrap();
        assert_eq!(tab["target"], "https://other.example");
        assert_eq!(tab["frame"], json!([0]));
    }

    #[test]
    fn recorder_json_preserves_non_main_target_and_frame() {
        let target = Target {
            selectors: vec![vec!["#pay".to_string()]],
            role: None,
            name: None,
            nth: None,
            test_id: None,
            input_type: None,
        };
        let step = Step::Click {
            target,
            count: 1,
            kind: ClickKind::Click,
            opens_popup: false,
            scope: Scope {
                target: "https://popup.example".to_string(),
                frame: vec![1, 0],
            },
            asserted_url: None,
        };
        let json = step.to_recorder_json();
        assert_eq!(json["target"], "https://popup.example");
        assert_eq!(json["frame"], json!([1, 0]));
    }

    #[test]
    fn popup_navigation_assertion_stays_out_of_recorder_json() {
        let target = Target {
            selectors: vec![vec!["#open".to_string()]],
            role: None,
            name: None,
            nth: None,
            test_id: None,
            input_type: None,
        };
        let step = Step::Click {
            target,
            count: 1,
            kind: ClickKind::Click,
            opens_popup: true,
            scope: Scope::default(),
            asserted_url: Some("https://popup.example".to_string()),
        };
        assert!(step.to_recorder_json().get("assertedEvents").is_none());
    }

    #[test]
    fn emits_the_implicit_viewport_once_for_many_actions() {
        let mut state = CodegenState::new();
        state.active = true;
        for action in ["navigate", "click", "hover"] {
            let command = if action == "navigate" {
                json!({ "url": "https://example.com" })
            } else {
                json!({ "selector": "#target" })
            };
            record_action(
                action,
                &command,
                &json!({}),
                &RefMap::new(),
                Some((1024, 768, 1.0, false)),
                Scope::default(),
                &mut state,
            )
            .unwrap();
        }
        assert_eq!(
            state
                .steps
                .iter()
                .filter(|step| matches!(step, Step::SetViewport { .. }))
                .count(),
            1
        );
    }
}
