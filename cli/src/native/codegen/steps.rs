use super::probe::{self, Probe};
use crate::native::cdp::client::CdpClient;
use crate::native::element::{parse_ref, RefMap};
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SelectorKind {
    TestId {
        value: String,
    },
    Role {
        role: String,
        name: String,
        nth: Option<usize>,
    },
    Css {
        value: String,
    },
    XPath {
        value: String,
    },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Target {
    pub selectors: Vec<SelectorKind>,
    pub input_type: Option<String>,
}

impl Target {
    fn has_safe_selector(&self) -> bool {
        self.selectors.iter().any(|selector| match selector {
            SelectorKind::TestId { value }
            | SelectorKind::Css { value }
            | SelectorKind::XPath { value } => !value.is_empty(),
            SelectorKind::Role { role, name, .. } => !role.is_empty() && !name.is_empty(),
        })
    }

    pub fn recorder_selectors(&self) -> Vec<Vec<String>> {
        self.selectors
            .iter()
            .filter_map(|selector| match selector {
                // The exact escaped CSS candidate from the probe is used for
                // test IDs. Building CSS from the raw value here can change
                // its meaning for quotes or other CSS syntax characters.
                SelectorKind::TestId { .. } => None,
                SelectorKind::Role { name, .. } if !name.is_empty() => {
                    Some(vec![format!("aria/{name}")])
                }
                SelectorKind::Css { value } if !value.is_empty() => Some(vec![value.clone()]),
                SelectorKind::XPath { value } if !value.is_empty() => {
                    Some(vec![format!("xpath/{value}")])
                }
                _ => None,
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Scope {
    pub target: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub frame: Vec<usize>,
}

#[derive(Clone, Debug)]
pub struct ActionContext {
    pub before: Scope,
    pub after: Option<Scope>,
}

impl From<Scope> for ActionContext {
    fn from(before: Scope) -> Self {
        Self {
            before,
            after: None,
        }
    }
}

impl Default for Scope {
    fn default() -> Self {
        Self {
            target: "main".to_string(),
            frame: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum ClickKind {
    Click,
    Check,
    Uncheck,
    Tap,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum PointerKind {
    Mouse,
    Touch,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub enum NavigationKind {
    Goto,
    Back,
    Forward,
    Reload,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
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
    ScopedViewport {
        width: i64,
        height: i64,
        device_scale_factor: f64,
        is_mobile: bool,
        scope: Scope,
    },
    ScopedNavigation {
        kind: NavigationKind,
        url: String,
        scope: Scope,
    },
    Pointer {
        target: Target,
        kind: ClickKind,
        pointer: PointerKind,
        button: String,
        count: u8,
        position: Option<(f64, f64)>,
        opens_popup: bool,
        #[serde(default)]
        popup_page: Option<String>,
        scope: Scope,
        asserted_url: Option<String>,
    },
    Fill {
        target: Target,
        value: String,
        scope: Scope,
        asserted_url: Option<String>,
    },
    SetValue {
        target: Target,
        value: String,
        scope: Scope,
        asserted_url: Option<String>,
    },
    Type {
        target: Target,
        text: String,
        clear: bool,
        delay_ms: Option<u64>,
        scope: Scope,
        asserted_url: Option<String>,
    },
    Select {
        target: Target,
        values: Vec<String>,
        scope: Scope,
        asserted_url: Option<String>,
    },
    Press {
        modifiers: Vec<String>,
        key: String,
        scope: Scope,
        asserted_url: Option<String>,
    },
    Wheel {
        target: Option<Target>,
        x: f64,
        y: f64,
        scope: Scope,
    },
    Upload {
        target: Target,
        paths: Vec<String>,
        scope: Scope,
    },
    NewPage {
        url: Option<String>,
        scope: Scope,
    },
    OpenPage {
        url: String,
        source_scope: Scope,
        scope: Scope,
    },
    ClosePage {
        scope: Scope,
    },
}

impl Step {
    pub fn has_password(&self) -> bool {
        matches!(self,
            Self::Change { target, .. }
            | Self::Fill { target, .. }
            | Self::SetValue { target, .. }
            | Self::Type { target, .. }
            if target.input_type.as_deref() == Some("password")
        )
    }

    pub fn to_recorder_json(&self) -> Value {
        let recorder_safe =
            |target: &Target| target.has_safe_selector() && !target.recorder_selectors().is_empty();
        let unsafe_target = match self {
            Self::Click { target, .. }
            | Self::Pointer { target, .. }
            | Self::Hover { target, .. }
            | Self::Change { target, .. }
            | Self::Fill { target, .. }
            | Self::SetValue { target, .. }
            | Self::Type { target, .. }
            | Self::Select { target, .. }
            | Self::Upload { target, .. }
            | Self::WaitForElement { target, .. } => !recorder_safe(target),
            Self::Scroll {
                target: Some(target),
                ..
            }
            | Self::Wheel {
                target: Some(target),
                ..
            } => !recorder_safe(target),
            _ => false,
        };
        if unsafe_target {
            return Value::Null;
        }
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
                    json!({ "type": "click", "selectors": target.recorder_selectors(), "offsetX": 0, "offsetY": 0, "button": "primary", "clickCount": count })
                } else {
                    with_navigation(
                        json!({ "type": "click", "selectors": target.recorder_selectors(), "offsetX": 0, "offsetY": 0, "button": "primary", "clickCount": count }),
                        asserted_url,
                    )
                },
                scope,
            ),
            Self::Hover { target, scope } => with_scope(
                json!({ "type": "hover", "selectors": target.recorder_selectors(), "offsetX": 0, "offsetY": 0 }),
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
                    json!({ "type": "change", "selectors": target.recorder_selectors(), "value": value }),
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
                    value["selectors"] = json!(target.recorder_selectors());
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
                let mut value =
                    json!({ "type": "waitForElement", "selectors": target.recorder_selectors() });
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
            Self::ScopedViewport {
                width,
                height,
                device_scale_factor,
                is_mobile,
                scope,
            } => with_scope(
                json!({ "type": "setViewport", "width": width, "height": height, "deviceScaleFactor": device_scale_factor, "isMobile": is_mobile, "hasTouch": is_mobile, "isLandscape": false }),
                scope,
            ),
            Self::ScopedNavigation { url, scope, .. } => {
                with_scope(json!({ "type": "navigate", "url": url }), scope)
            }
            Self::Pointer {
                target,
                count,
                button,
                position,
                asserted_url,
                scope,
                opens_popup,
                ..
            } => {
                let (offset_x, offset_y) = position.unwrap_or((0.0, 0.0));
                let value = json!({ "type": "click", "selectors": target.recorder_selectors(), "offsetX": offset_x, "offsetY": offset_y, "button": recorder_button(button), "clickCount": count });
                with_scope(
                    if *opens_popup {
                        value
                    } else {
                        with_navigation(value, asserted_url)
                    },
                    scope,
                )
            }
            Self::Fill {
                target,
                value,
                scope,
                asserted_url,
            }
            | Self::SetValue {
                target,
                value,
                scope,
                asserted_url,
            } => with_scope(
                with_navigation(
                    json!({ "type": "change", "selectors": target.recorder_selectors(), "value": value }),
                    asserted_url,
                ),
                scope,
            ),
            Self::Select {
                target,
                values,
                scope,
                asserted_url,
            } if values.len() == 1 => with_scope(
                with_navigation(
                    json!({ "type": "change", "selectors": target.recorder_selectors(), "value": values[0] }),
                    asserted_url,
                ),
                scope,
            ),
            Self::Wheel {
                target,
                x,
                y,
                scope,
            } => {
                let mut value = json!({ "type": "scroll", "x": x, "y": y });
                if let Some(target) = target {
                    value["selectors"] = json!(target.recorder_selectors());
                }
                with_scope(value, scope)
            }
            Self::ClosePage { scope } => with_scope(json!({ "type": "close" }), scope),
            Self::Type { .. }
            | Self::Select { .. }
            | Self::Press { .. }
            | Self::Upload { .. }
            | Self::NewPage { .. }
            | Self::OpenPage { .. } => Value::Null,
        }
    }
}

fn recorder_button(button: &str) -> &str {
    match button {
        "right" => "secondary",
        "middle" => "auxiliary",
        _ => "primary",
    }
}

fn with_navigation(mut step: Value, url: &Option<String>) -> Value {
    if let Some(url) = url {
        step["assertedEvents"] = json!([{ "type": "navigation", "url": url }]);
    }
    step
}

fn with_scope(mut step: Value, scope: &Scope) -> Value {
    if scope.target != "main" && scope.target != "p1" {
        step["target"] = json!(scope.target);
    }
    if !scope.frame.is_empty() {
        step["frame"] = json!(scope.frame);
    }
    step
}

fn recorder_issue(step: &Step) -> Option<(&'static str, &'static str, bool)> {
    match step {
        Step::Type { .. } => Some((
            "recorder-type-omitted",
            "Recorder JSON cannot represent sequential typing or typing delay.",
            true,
        )),
        Step::Select { values, .. } if values.len() != 1 => Some((
            "recorder-multi-select-omitted",
            "Recorder JSON cannot represent a multi-value select.",
            true,
        )),
        Step::Press { .. } => Some((
            "recorder-key-chord-omitted",
            "Recorder JSON cannot represent a key chord as one exact action.",
            true,
        )),
        Step::Upload { .. } => Some((
            "recorder-upload-omitted",
            "Recorder JSON cannot represent file upload.",
            true,
        )),
        Step::NewPage { .. } | Step::OpenPage { .. } | Step::NewTab { .. } => Some((
            "recorder-page-create-omitted",
            "Recorder JSON cannot represent explicit page creation.",
            true,
        )),
        Step::ScopedNavigation {
            kind: NavigationKind::Back | NavigationKind::Forward | NavigationKind::Reload,
            ..
        } => Some((
            "recorder-history-navigation-lossy",
            "Recorder JSON converts back, forward, or reload to navigation to the observed URL.",
            false,
        )),
        _ if step.to_recorder_json().is_null() => Some((
            "recorder-step-omitted",
            "Recorder JSON cannot safely represent this captured step.",
            true,
        )),
        _ => None,
    }
}

pub fn recorder_issue_for_step(step: &Step) -> Option<(&'static str, &'static str, bool)> {
    recorder_issue(step)
}

fn selector_target(selector: &str, refs: &RefMap) -> Target {
    if let Some(reference) = parse_ref(selector).and_then(|reference| refs.get(&reference)) {
        let mut selectors = Vec::new();
        if !reference.role.is_empty() && !reference.name.is_empty() {
            selectors.push(SelectorKind::Role {
                role: reference.role.clone(),
                name: reference.name.clone(),
                nth: reference.nth,
            });
        }
        if let Some(value) = reference.selector.clone() {
            selectors.push(SelectorKind::Css { value });
        }
        return Target {
            selectors,
            input_type: None,
        };
    }
    let selector = if selector.starts_with("text=") || selector.starts_with("//") {
        None
    } else if let Some(xpath) = selector.strip_prefix("xpath=") {
        Some(SelectorKind::XPath {
            value: xpath.to_string(),
        })
    } else {
        Some(SelectorKind::Css {
            value: selector.to_string(),
        })
    };
    Target {
        selectors: selector.into_iter().collect(),
        input_type: None,
    }
}

fn target(cmd: &Value, refs: &RefMap, capture: Option<&probe::ElementCapture>) -> Option<Target> {
    let selector = cmd.get("selector").and_then(Value::as_str)?;
    if capture.is_some_and(|capture| capture.frame_probe_failed) {
        return None;
    }
    let mut target = selector_target(selector, refs);
    if let Some(probe) = capture.and_then(|capture| capture.probe.as_ref()) {
        enrich_target(&mut target, probe);
    }
    (!target.selectors.is_empty()).then_some(target)
}

fn select_values(cmd: &Value) -> Vec<String> {
    match cmd.get("values") {
        Some(Value::Array(values)) => values
            .iter()
            .filter_map(Value::as_str)
            .map(str::to_string)
            .collect(),
        Some(Value::String(value)) => vec![value.clone()],
        _ => cmd
            .get("value")
            .and_then(Value::as_str)
            .map(|value| vec![value.to_string()])
            .unwrap_or_default(),
    }
}

fn normalize_key_chord(value: &str) -> (Vec<String>, String) {
    let parts = value.split('+').collect::<Vec<_>>();
    if parts.len() < 2 {
        return (Vec::new(), value.to_string());
    }
    let mut modifiers = Vec::new();
    let mut key = Vec::new();
    for part in parts {
        let normalized = match part.to_ascii_lowercase().as_str() {
            "alt" => Some("Alt"),
            "control" | "ctrl" => Some("Control"),
            "meta" | "cmd" | "command" => Some("Meta"),
            "shift" => Some("Shift"),
            _ => None,
        };
        if let Some(modifier) = normalized {
            if !modifiers.iter().any(|existing| existing == modifier) {
                modifiers.push(modifier.to_string());
            }
        } else {
            key.push(part);
        }
    }
    if modifiers.is_empty() || key.is_empty() {
        (Vec::new(), value.to_string())
    } else {
        (modifiers, key.join("+"))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ActionSupport {
    Recorded,
    Observation,
    Omitted,
}

fn action_support(action: &str) -> ActionSupport {
    // This support matrix is the capture contract. New successful commands
    // must be classified here so they cannot disappear from a flow silently.
    match action {
        "navigate" | "back" | "forward" | "reload" | "click" | "tap" | "dblclick" | "check"
        | "uncheck" | "hover" | "fill" | "setvalue" | "type" | "select" | "upload" | "press"
        | "scroll" | "viewport" | "isvisible" | "isenabled" | "ischecked" | "count" | "wait"
        | "tab_new" | "tab_close" => ActionSupport::Recorded,
        "snapshot" | "screenshot" | "gettext" | "getattribute" | "url" | "cdp_url" | "title"
        | "content" | "read" | "console" | "errors" | "inspect" | "boundingbox" | "innertext"
        | "innerhtml" | "inputvalue" | "styles" | "cookies_get" | "storage_get"
        | "session_info" | "state_save" | "credentials_get" | "credentials_list"
        | "credentials_delete" | "download" | "diff_snapshot" | "diff_url" | "responsebody"
        | "requests" | "request_detail" | "react_tree" | "react_inspect" | "react_suspense"
        | "vitals" | "tab_list" | "tab_switch" | "stream_status" | "device_list"
        | "codegen_start" | "codegen_stop" | "codegen_status" | "codegen_discard" | "launch"
        | "close" | "webmcp_list" | "webmcp_result" => ActionSupport::Observation,
        _ => ActionSupport::Omitted,
    }
}

#[allow(clippy::too_many_arguments)]
pub fn record_action<C: Into<ActionContext>>(
    action: &str,
    cmd: &Value,
    data: &Value,
    refs: &RefMap,
    viewport: Option<(i32, i32, f64, bool)>,
    context: C,
    capture: Option<&probe::ElementCapture>,
    state: &mut super::CodegenState,
) -> Result<(), String> {
    let mut steps = Vec::new();
    let mut omission = None;
    let context = context.into();
    let mut sc = context.before;
    if let Some(frame) = capture.and_then(|capture| capture.frame.as_ref()) {
        sc.frame = frame.clone();
    }
    match action {
        "navigate" => {
            if let Some(url) = cmd.get("url").and_then(Value::as_str) {
                if state.page_url(&sc.target) != Some(url) {
                    steps.push(Step::ScopedNavigation {
                        kind: NavigationKind::Goto,
                        url: url.to_string(),
                        scope: sc.clone(),
                    });
                    state.update_page_url(&sc.target, url);
                }
            }
        }
        "back" | "forward" | "reload" => {
            if let Some(url) = data.get("url").and_then(Value::as_str) {
                steps.push(Step::ScopedNavigation {
                    kind: match action {
                        "back" => NavigationKind::Back,
                        "forward" => NavigationKind::Forward,
                        _ => NavigationKind::Reload,
                    },
                    url: url.to_string(),
                    scope: sc.clone(),
                });
                state.update_page_url(&sc.target, url);
            }
        }
        "click" | "tap" | "dblclick" | "check" | "uncheck" => {
            if action == "click" && cmd.get("newTab").and_then(Value::as_bool).unwrap_or(false) {
                let url = data
                    .get("url")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "The new page URL is not available for codegen.".to_string())?;
                steps.push(Step::OpenPage {
                    url: url.to_string(),
                    source_scope: sc.clone(),
                    scope: context.after.clone().unwrap_or_else(|| sc.clone()),
                });
            } else if let Some(target) = target(cmd, refs, capture) {
                let kind = match action {
                    "check" => ClickKind::Check,
                    "uncheck" => ClickKind::Uncheck,
                    "tap" => ClickKind::Tap,
                    _ => ClickKind::Click,
                };
                steps.push(Step::Pointer {
                    target,
                    kind,
                    pointer: if action == "tap" {
                        PointerKind::Touch
                    } else {
                        PointerKind::Mouse
                    },
                    button: cmd
                        .get("button")
                        .and_then(Value::as_str)
                        .unwrap_or("left")
                        .to_string(),
                    count: if action == "dblclick" {
                        2
                    } else {
                        cmd.get("clickCount").and_then(Value::as_u64).unwrap_or(1) as u8
                    },
                    position: capture.and_then(|capture| capture.position),
                    opens_popup: false,
                    popup_page: None,
                    scope: sc.clone(),
                    asserted_url: None,
                });
            } else {
                omission = Some("The action target did not have a safe selector.");
            }
        }
        "hover" => {
            if let Some(target) = target(cmd, refs, capture) {
                steps.push(Step::Hover {
                    target,
                    scope: sc.clone(),
                });
            } else {
                omission = Some("The action target did not have a safe selector.");
            }
        }
        "fill" | "setvalue" | "type" | "select" => {
            if let Some(target) = target(cmd, refs, capture) {
                match action {
                    "fill" => steps.push(Step::Fill {
                        target,
                        value: cmd
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        scope: sc.clone(),
                        asserted_url: None,
                    }),
                    "setvalue" => steps.push(Step::SetValue {
                        target,
                        value: cmd
                            .get("value")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        scope: sc.clone(),
                        asserted_url: None,
                    }),
                    "type" => steps.push(Step::Type {
                        target,
                        text: cmd
                            .get("text")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_string(),
                        clear: cmd.get("clear").and_then(Value::as_bool).unwrap_or(false),
                        delay_ms: cmd.get("delay").and_then(Value::as_u64),
                        scope: sc.clone(),
                        asserted_url: None,
                    }),
                    _ => steps.push(Step::Select {
                        target,
                        values: select_values(cmd),
                        scope: sc.clone(),
                        asserted_url: None,
                    }),
                }
            } else {
                omission = Some("The action target did not have a safe selector.");
            }
        }
        "upload" => {
            if let Some(target) = target(cmd, refs, capture) {
                let paths = cmd
                    .get("files")
                    .and_then(Value::as_array)
                    .map(|paths| {
                        paths
                            .iter()
                            .filter_map(Value::as_str)
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .or_else(|| {
                        cmd.get("file")
                            .and_then(Value::as_str)
                            .map(|path| vec![path.to_string()])
                    })
                    .unwrap_or_default();
                steps.push(Step::Upload {
                    target,
                    paths,
                    scope: sc.clone(),
                });
            } else {
                omission = Some("The action target did not have a safe selector.");
            }
        }
        "press" => {
            if let Some(key) = cmd.get("key").and_then(Value::as_str) {
                let (modifiers, key) = normalize_key_chord(key);
                steps.push(Step::Press {
                    modifiers,
                    key,
                    scope: sc.clone(),
                    asserted_url: None,
                });
            }
        }
        "scroll" => steps.push(Step::Wheel {
            target: target(cmd, refs, capture),
            x: capture
                .and_then(|capture| capture.scroll_delta)
                .map(|(x, _)| x)
                .unwrap_or_else(|| cmd.get("x").and_then(Value::as_f64).unwrap_or(0.0)),
            y: capture
                .and_then(|capture| capture.scroll_delta)
                .map(|(_, y)| y)
                .unwrap_or_else(|| cmd.get("y").and_then(Value::as_f64).unwrap_or(0.0)),
            scope: sc.clone(),
        }),
        "viewport" => {
            if let (Some(width), Some(height)) = (
                cmd.get("width").and_then(Value::as_i64),
                cmd.get("height").and_then(Value::as_i64),
            ) {
                steps.push(Step::ScopedViewport {
                    width,
                    height,
                    device_scale_factor: cmd
                        .get("deviceScaleFactor")
                        .and_then(Value::as_f64)
                        .unwrap_or(1.0),
                    is_mobile: cmd.get("mobile").and_then(Value::as_bool).unwrap_or(false),
                    scope: sc.clone(),
                });
            }
        }
        "isvisible" | "isenabled" | "ischecked" | "count" | "wait" => {
            if let Some(target) = target(cmd, refs, capture) {
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
                    scope: sc.clone(),
                    visible,
                    properties,
                    count,
                    operator: count.map(|_| "==".to_string()),
                });
            } else {
                omission = Some("The action target did not have a safe selector.");
            }
        }
        "close" => return Ok(()),
        "tab_new" => steps.push(Step::NewPage {
            url: cmd.get("url").and_then(Value::as_str).map(str::to_string),
            scope: context.after.clone().unwrap_or_else(|| sc.clone()),
        }),
        "tab_close" => steps.push(Step::ClosePage { scope: sc.clone() }),
        "tab_switch" => return Ok(()),
        "snapshot" | "screenshot" | "gettext" | "getattribute" | "url" | "title" | "content"
        | "console" | "errors" | "inspect" | "boundingbox" | "innertext" | "innerhtml"
        | "inputvalue" | "styles" | "codegen_status" => return Ok(()),
        _ if action_support(action) == ActionSupport::Observation => return Ok(()),
        _ => omission = Some("The successful action is not supported by codegen."),
    }
    if !steps.is_empty() && !state.viewport_emitted {
        let (width, height, scale, mobile) = viewport.unwrap_or((1280, 720, 1.0, false));
        steps.insert(
            0,
            Step::ScopedViewport {
                width: width.into(),
                height: height.into(),
                device_scale_factor: scale,
                is_mobile: mobile,
                scope: sc.clone(),
            },
        );
        state.viewport_emitted = true;
    }
    if !steps.is_empty() || omission.is_some() {
        let action_id = state.capture_action(action, steps);
        state.register_action_intent(action_id, &sc.target);
        if let Some(message) = omission {
            state.capture_warning("omitted-action", message, Some(action_id));
        }
        if capture.is_some_and(|capture| capture.probe_failed) {
            state.capture_warning(
                "selector-probe-failed",
                "Codegen could not inspect the action target before the action.",
                Some(action_id),
            );
        }
        if matches!(action, "fill" | "setvalue" | "type" | "select" | "upload") {
            state.capture_warning(
                "typed-values-stored",
                "The recording stores action values verbatim.",
                Some(action_id),
            );
        }
        if capture
            .and_then(|capture| capture.probe.as_ref())
            .and_then(|probe| probe.input_type.as_deref())
            == Some("password")
        {
            state.capture_warning(
                "password-value-stored",
                "The recording stores a confirmed password value verbatim.",
                Some(action_id),
            );
        }
    }
    Ok(())
}

fn enrich_target(target: &mut Target, probe: &Probe) {
    if let Some(selector) = &probe.selector {
        let candidate = SelectorKind::Css {
            value: selector.clone(),
        };
        if !target.selectors.contains(&candidate) {
            target.selectors.push(candidate);
        }
    }
    if let Some(test_id) = &probe.test_id {
        target.selectors.insert(
            0,
            SelectorKind::TestId {
                value: test_id.clone(),
            },
        );
    }
    target.input_type = probe.input_type.clone();
}

fn enrich_step(step: &mut Step, probe: &Probe) {
    match step {
        Step::Click { target, .. }
        | Step::Pointer { target, .. }
        | Step::Hover { target, .. }
        | Step::Change { target, .. }
        | Step::Fill { target, .. }
        | Step::SetValue { target, .. }
        | Step::Type { target, .. }
        | Step::Select { target, .. }
        | Step::Upload { target, .. }
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
        Step::Click { asserted_url, .. }
        | Step::Pointer { asserted_url, .. }
        | Step::Change { asserted_url, .. }
        | Step::Fill { asserted_url, .. }
        | Step::SetValue { asserted_url, .. }
        | Step::Type { asserted_url, .. }
        | Step::Select { asserted_url, .. }
        | Step::Press { asserted_url, .. } => *asserted_url = Some(url.to_string()),
        _ => {}
    }
}

pub fn can_assert_navigation(step: &Step) -> bool {
    matches!(
        step,
        Step::Click { .. }
            | Step::Pointer { .. }
            | Step::Change { .. }
            | Step::Fill { .. }
            | Step::SetValue { .. }
            | Step::Type { .. }
            | Step::Select { .. }
            | Step::Press { .. }
    )
}

pub fn action_can_navigate(action: &str) -> bool {
    matches!(
        action,
        "click"
            | "tap"
            | "dblclick"
            | "check"
            | "uncheck"
            | "fill"
            | "setvalue"
            | "type"
            | "select"
            | "press"
    )
}

pub fn can_open_popup(step: &Step) -> bool {
    matches!(
        step,
        Step::Click { .. }
            | Step::Pointer {
                pointer: PointerKind::Mouse,
                ..
            }
    )
}

pub fn step_scope(step: &Step) -> Option<&Scope> {
    match step {
        Step::Click { scope, .. }
        | Step::Hover { scope, .. }
        | Step::Change { scope, .. }
        | Step::KeyDown { scope, .. }
        | Step::KeyUp { scope, .. }
        | Step::Scroll { scope, .. }
        | Step::WaitForElement { scope, .. }
        | Step::ScopedViewport { scope, .. }
        | Step::ScopedNavigation { scope, .. }
        | Step::Pointer { scope, .. }
        | Step::Fill { scope, .. }
        | Step::SetValue { scope, .. }
        | Step::Type { scope, .. }
        | Step::Select { scope, .. }
        | Step::Press { scope, .. }
        | Step::Wheel { scope, .. }
        | Step::Upload { scope, .. }
        | Step::NewPage { scope, .. }
        | Step::OpenPage { scope, .. }
        | Step::ClosePage { scope } => Some(scope),
        Step::SetViewport { .. } | Step::Navigate { .. } | Step::Close | Step::NewTab { .. } => {
            None
        }
    }
}

pub fn mark_popup(step: &mut Step) {
    match step {
        Step::Click { opens_popup, .. } | Step::Pointer { opens_popup, .. } => *opens_popup = true,
        _ => {}
    }
}

pub fn bind_popup(step: &mut Step, page_id: &str) {
    match step {
        Step::Click { opens_popup, .. } => *opens_popup = true,
        Step::Pointer {
            opens_popup,
            popup_page,
            ..
        } => {
            *opens_popup = true;
            *popup_page = Some(page_id.to_string());
        }
        _ => {}
    }
}

pub fn set_frame_scope(steps: &mut [Step], frame: Vec<usize>) {
    for step in steps {
        match step {
            Step::Click { scope, .. }
            | Step::Pointer { scope, .. }
            | Step::Hover { scope, .. }
            | Step::Change { scope, .. }
            | Step::Fill { scope, .. }
            | Step::SetValue { scope, .. }
            | Step::Type { scope, .. }
            | Step::Select { scope, .. }
            | Step::Press { scope, .. }
            | Step::Wheel { scope, .. }
            | Step::Upload { scope, .. }
            | Step::ScopedNavigation { scope, .. }
            | Step::ScopedViewport { scope, .. }
            | Step::NewPage { scope, .. }
            | Step::OpenPage { scope, .. }
            | Step::ClosePage { scope, .. }
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
        | Step::Pointer { scope, .. }
        | Step::Hover { scope, .. }
        | Step::Change { scope, .. }
        | Step::Fill { scope, .. }
        | Step::SetValue { scope, .. }
        | Step::Type { scope, .. }
        | Step::Select { scope, .. }
        | Step::Press { scope, .. }
        | Step::Wheel { scope, .. }
        | Step::Upload { scope, .. }
        | Step::ScopedNavigation { scope, .. }
        | Step::ScopedViewport { scope, .. }
        | Step::NewPage { scope, .. }
        | Step::OpenPage { scope, .. }
        | Step::ClosePage { scope, .. }
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
    use crate::native::codegen::{CodegenState, CodegenStatus};

    fn active_state() -> (tempfile::TempDir, CodegenState) {
        let directory = tempfile::tempdir().unwrap();
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        state.sidecar_path = Some(directory.path().join("flow.jsonl"));
        std::fs::write(state.sidecar_path.as_ref().unwrap(), "").unwrap();
        (directory, state)
    }

    #[test]
    fn captures_core_steps_and_skips_observations() {
        let dir = tempfile::tempdir().unwrap();
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
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
            None,
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
            None,
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
            None,
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
            None,
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
        state.status = CodegenStatus::Active;
        let refs = RefMap::new();
        record_action(
            "navigate",
            &json!({ "url": "https://example.com" }),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            None,
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
            None,
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
            None,
            &mut state,
        )
        .unwrap();
        assert_eq!(state.steps.len(), 3);
        assert_eq!(state.steps[2].to_recorder_json()["visible"], false);
    }

    #[test]
    fn observation_before_first_action_emits_no_viewport() {
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        record_action(
            "snapshot",
            &json!({}),
            &json!({}),
            &RefMap::new(),
            Some((1280, 720, 1.0, false)),
            Scope::default(),
            None,
            &mut state,
        )
        .unwrap();
        assert!(state.steps.is_empty());
        assert!(!state.viewport_emitted);
    }

    #[test]
    fn production_refs_use_a_probed_selector_when_accessible_name_is_empty() {
        let mut refs = RefMap::new();
        refs.add_with_frame("e1".into(), Some(42), "button", "", None, None);
        let capture = probe::ElementCapture {
            probe: Some(Probe {
                selector: Some("#actual".to_string()),
                ..Probe::default()
            }),
            ..probe::ElementCapture::default()
        };
        let mut captured = CodegenState::new();
        captured.status = CodegenStatus::Active;
        record_action(
            "click",
            &json!({ "selector": "@e1" }),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            Some(&capture),
            &mut captured,
        )
        .unwrap();
        assert!(matches!(
            captured.steps.last(),
            Some(Step::Pointer { target, .. }) if target.selectors == vec![SelectorKind::Css { value: "#actual".to_string() }]
        ));

        let mut omitted = CodegenState::new();
        omitted.status = CodegenStatus::Active;
        record_action(
            "click",
            &json!({ "selector": "@e1" }),
            &json!({}),
            &refs,
            None,
            Scope::default(),
            Some(&probe::ElementCapture {
                probe_failed: true,
                ..probe::ElementCapture::default()
            }),
            &mut omitted,
        )
        .unwrap();
        assert!(omitted.steps.is_empty());
        assert!(omitted
            .capture_errors
            .iter()
            .any(|warning| warning.starts_with("omitted-action:")));

        let mut named_refs = RefMap::new();
        named_refs.add_with_frame("e2".into(), Some(43), "button", "Save", None, None);
        let mut named = CodegenState::new();
        named.status = CodegenStatus::Active;
        record_action(
            "click",
            &json!({ "selector": "@e2" }),
            &json!({}),
            &named_refs,
            None,
            Scope::default(),
            Some(&probe::ElementCapture {
                probe_failed: true,
                ..probe::ElementCapture::default()
            }),
            &mut named,
        )
        .unwrap();
        assert!(matches!(
            named.steps.last(),
            Some(Step::Pointer { target, .. }) if matches!(target.selectors.first(), Some(SelectorKind::Role { role, name, .. }) if role == "button" && name == "Save")
        ));
    }

    #[test]
    fn css_password_capture_adds_security_warnings() {
        let capture = probe::ElementCapture {
            probe: Some(Probe {
                selector: Some("#password".to_string()),
                input_type: Some("password".to_string()),
                ..Probe::default()
            }),
            ..probe::ElementCapture::default()
        };
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        record_action(
            "fill",
            &json!({ "selector": "#password", "value": "not-printed" }),
            &json!({}),
            &RefMap::new(),
            None,
            Scope::default(),
            Some(&capture),
            &mut state,
        )
        .unwrap();

        assert!(state
            .capture_errors
            .iter()
            .any(|warning| warning.starts_with("typed-values-stored:")));
        assert!(state
            .capture_errors
            .iter()
            .any(|warning| warning.starts_with("password-value-stored:")));
        assert!(!state
            .capture_errors
            .iter()
            .any(|warning| warning.contains("not-printed")));
    }

    #[test]
    fn captures_supported_actions_with_production_selector_forms() {
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        let refs = RefMap::new();
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
            ("click", json!({ "selector": "#button" }), json!({})),
            ("tap", json!({ "selector": "#tap" }), json!({})),
            (
                "dblclick",
                json!({ "selector": "xpath=//button" }),
                json!({}),
            ),
            ("hover", json!({ "selector": "a" }), json!({})),
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
                None,
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
        assert_eq!(recorder_steps[4]["selectors"][0][0], "#button");
        assert_eq!(recorder_steps[6]["selectors"][0][0], "xpath///button");
        assert!(types.contains(&"hover"));
        assert!(types.contains(&"change"));
        assert!(types.contains(&"scroll"));
        assert!(types.contains(&"waitForElement"));
        assert_eq!(
            state
                .steps
                .iter()
                .filter(|step| matches!(
                    step,
                    Step::SetViewport { .. } | Step::ScopedViewport { .. }
                ))
                .count(),
            2,
            "one initial viewport plus the explicit viewport action"
        );
        assert!(state.steps.iter().any(|step| matches!(step, Step::Pointer { target, .. } if target.selectors == vec![SelectorKind::Css { value: "#button".to_string() }])));
    }

    #[test]
    fn explicit_link_opening_creates_a_page_without_a_popup_click() {
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        state.viewport_emitted = true;
        record_action(
            "click",
            &json!({ "selector": "#docs", "newTab": true }),
            &json!({ "url": "https://example.com/docs", "tabId": "t2" }),
            &RefMap::new(),
            None,
            ActionContext {
                before: Scope {
                    target: "p1".to_string(),
                    frame: Vec::new(),
                },
                after: Some(Scope {
                    target: "p2".to_string(),
                    frame: Vec::new(),
                }),
            },
            None,
            &mut state,
        )
        .unwrap();

        assert!(matches!(
            state.steps.as_slice(),
            [Step::OpenPage { url, source_scope, scope }]
                if url == "https://example.com/docs"
                    && source_scope.target == "p1"
                    && scope.target == "p2"
        ));
        assert!(state.steps[0].to_recorder_json().is_null());
    }

    #[test]
    fn tab_close_uses_the_pre_action_scope() {
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        state.viewport_emitted = true;
        record_action(
            "tab_close",
            &json!({}),
            &json!({ "tabId": "t2" }),
            &RefMap::new(),
            None,
            ActionContext {
                before: Scope {
                    target: "p2".to_string(),
                    frame: Vec::new(),
                },
                after: Some(Scope {
                    target: "p1".to_string(),
                    frame: Vec::new(),
                }),
            },
            None,
            &mut state,
        )
        .unwrap();

        assert!(matches!(
            state.steps.as_slice(),
            [Step::ClosePage { scope }] if scope.target == "p2"
        ));
    }

    #[test]
    fn ignores_observations_without_a_recorder_mapping() {
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
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
                None,
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
            selectors: vec![SelectorKind::Css {
                value: "#pay".to_string(),
            }],
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
            selectors: vec![SelectorKind::Css {
                value: "#open".to_string(),
            }],
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
        state.status = CodegenStatus::Active;
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
                None,
                &mut state,
            )
            .unwrap();
        }
        assert_eq!(
            state
                .steps
                .iter()
                .filter(|step| matches!(
                    step,
                    Step::SetViewport { .. } | Step::ScopedViewport { .. }
                ))
                .count(),
            1
        );
    }

    #[test]
    fn typed_actions_keep_exact_action_intent_and_page_scope() {
        let capture = probe::ElementCapture {
            probe: Some(Probe {
                selector: Some("#target".to_string()),
                ..Probe::default()
            }),
            position: Some((12.5, 24.0)),
            scroll_delta: Some((0.0, 450.0)),
            ..probe::ElementCapture::default()
        };
        let mut state = CodegenState::new();
        state.status = CodegenStatus::Active;
        for (action, command) in [
            (
                "select",
                json!({ "selector": "#target", "values": ["a", "b"] }),
            ),
            (
                "type",
                json!({ "selector": "#target", "text": "text", "clear": true, "delay": 25 }),
            ),
            ("press", json!({ "key": "Control+Shift+a" })),
            (
                "click",
                json!({ "selector": "#target", "button": "right", "clickCount": 2 }),
            ),
            (
                "scroll",
                json!({ "selector": "#target", "direction": "down", "amount": 450 }),
            ),
            (
                "upload",
                json!({ "selector": "#target", "files": ["a.txt", "b.txt"] }),
            ),
            (
                "viewport",
                json!({ "width": 900, "height": 700, "deviceScaleFactor": 2.0, "mobile": true }),
            ),
        ] {
            record_action(
                action,
                &command,
                &json!({}),
                &RefMap::new(),
                None,
                Scope {
                    target: "page-2".to_string(),
                    frame: vec![1],
                },
                Some(&capture),
                &mut state,
            )
            .unwrap();
        }

        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Select { values, .. } if values == &vec!["a".to_string(), "b".to_string()]
        )));
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Type {
                clear: true,
                delay_ms: Some(25),
                ..
            }
        )));
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Press { modifiers, key, .. } if modifiers == &vec!["Control".to_string(), "Shift".to_string()] && key == "a"
        )));
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Pointer { button, count: 2, position: Some((12.5, 24.0)), .. } if button == "right"
        )));
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Wheel { y, .. } if *y == 450.0
        )));
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Upload { paths, .. } if paths == &vec!["a.txt".to_string(), "b.txt".to_string()]
        )));
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::ScopedViewport { width: 900, height: 700, device_scale_factor, is_mobile: true, scope }
                if *device_scale_factor == 2.0 && scope.target == "page-2" && scope.frame == vec![1]
        )));
        for step in &mut state.steps {
            attach_navigation(step, "https://example.com/done");
        }
        assert!(state.steps.iter().any(|step| matches!(
            step,
            Step::Press { asserted_url: Some(url), scope, .. }
                if url == "https://example.com/done" && scope.target == "page-2"
        )));
    }

    #[test]
    fn support_matrix_warns_for_unsupported_successful_mutation() {
        let (_directory, mut state) = active_state();
        record_action(
            "drag",
            &json!({ "source": "#a", "target": "#b" }),
            &json!({ "dragged": true }),
            &RefMap::new(),
            None,
            Scope::default(),
            None,
            &mut state,
        )
        .unwrap();

        assert_eq!(state.steps.len(), 0);
        assert_eq!(state.capture_errors.len(), 1);
        assert!(state.capture_errors[0].starts_with("omitted-action:"));
    }

    #[test]
    fn support_matrix_keeps_read_only_commands_out_of_capture() {
        for action in [
            "snapshot",
            "url",
            "cookies_get",
            "storage_get",
            "requests",
            "vitals",
            "tab_list",
            "webmcp_list",
            "webmcp_result",
        ] {
            let (_directory, mut state) = active_state();
            record_action(
                action,
                &json!({}),
                &json!({}),
                &RefMap::new(),
                None,
                Scope::default(),
                None,
                &mut state,
            )
            .unwrap();
            assert!(state.steps.is_empty(), "{action} produced a step");
            assert!(
                state.capture_errors.is_empty(),
                "{action} produced a warning"
            );
        }
    }
}
