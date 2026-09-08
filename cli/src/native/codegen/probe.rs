use crate::native::cdp::client::CdpClient;
use crate::native::cdp::types::{
    CallFunctionOnParams, DomResolveNodeParams, DomResolveNodeResult, EvaluateResult,
};
use crate::native::element::ResolvedElement;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
pub struct Probe {
    pub selector: Option<String>,
    pub test_id: Option<String>,
    pub href: Option<String>,
    pub input_type: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ElementCapture {
    pub probe: Option<Probe>,
    pub frame: Option<Vec<usize>>,
    pub probe_failed: bool,
    pub frame_probe_failed: bool,
    pub position: Option<(f64, f64)>,
    pub scroll_delta: Option<(f64, f64)>,
    /// Absolute scroll position after a scroll command. Recorder replays a
    /// scroll as a position, not as a delta.
    #[serde(default)]
    pub scroll_position: Option<(f64, f64)>,
    /// Whether a check or uncheck command actually changed the control. A
    /// command that changed nothing must not become a Recorder click, because
    /// replay would then clear the control.
    #[serde(default)]
    pub state_changed: Option<bool>,
}

const ELEMENT_PROBE: &str = r#"function() {
  const esc = (value) => CSS.escape(value);
  const unique = (selector) => { try { return document.querySelectorAll(selector).length === 1; } catch { return false; } };
  const testId = this.getAttribute('data-testid');
  const usableTestId = testId && unique(`[data-testid="${esc(testId)}"]`) ? testId : null;
  const selector = (() => {
    if (usableTestId) return `[data-testid="${esc(usableTestId)}"]`;
    if (this.id && unique(`#${esc(this.id)}`)) return `#${esc(this.id)}`;
    const parts = [];
    let node = this;
    while (node && node.nodeType === Node.ELEMENT_NODE && node !== document.documentElement) {
      let part = node.localName;
      let index = 1;
      let sibling = node.previousElementSibling;
      while (sibling) { if (sibling.localName === node.localName) index++; sibling = sibling.previousElementSibling; }
      part += `:nth-of-type(${index})`;
      parts.unshift(part);
      const candidate = parts.join(' > ');
      if (unique(candidate)) return candidate;
      node = node.parentElement;
    }
    return parts.join(' > ');
  })();
  return { selector, testId: usableTestId, href: location.href, type: this instanceof HTMLInputElement ? this.type : null };
}"#;

pub async fn probe_element(
    client: &CdpClient,
    session_id: &str,
    backend_node_id: i64,
) -> Result<Probe, String> {
    let resolved: DomResolveNodeResult = client
        .send_command_typed(
            "DOM.resolveNode",
            &DomResolveNodeParams {
                backend_node_id: Some(backend_node_id),
                node_id: None,
                object_group: Some("agent-browser-codegen".to_string()),
            },
            Some(session_id),
        )
        .await?;
    let object_id = resolved
        .object
        .object_id
        .ok_or("Could not resolve codegen element")?;
    probe_element_object(client, session_id, &object_id).await
}

/// Probe the exact DOM object that an interaction already resolved.
pub async fn probe_element_object(
    client: &CdpClient,
    session_id: &str,
    object_id: &str,
) -> Result<Probe, String> {
    let result: EvaluateResult = client
        .send_command_typed(
            "Runtime.callFunctionOn",
            &CallFunctionOnParams {
                function_declaration: ELEMENT_PROBE.to_string(),
                object_id: Some(object_id.to_string()),
                arguments: None,
                return_by_value: Some(true),
                await_promise: Some(false),
            },
            Some(session_id),
        )
        .await?;
    if let Some(details) = result.exception_details {
        let message = details
            .exception
            .as_ref()
            .and_then(|exception| exception.description.as_deref())
            .unwrap_or(&details.text);
        return Err(format!("Codegen element probe failed: {message}"));
    }
    let value = result.result.value.unwrap_or_default();
    Ok(probe_from_value(&value))
}

fn probe_from_value(value: &serde_json::Value) -> Probe {
    Probe {
        selector: value
            .get("selector")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        test_id: value
            .get("testId")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        href: value
            .get("href")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        input_type: value
            .get("type")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

/// Capture selector and frame facts before an action can replace the element.
/// Probe errors are capture warnings and never change browser action success.
pub async fn capture_resolved_element(
    client: &CdpClient,
    top_session_id: &str,
    resolved: &ResolvedElement,
) -> ElementCapture {
    let probe = probe_element_object(client, &resolved.session_id, &resolved.object_id).await;
    let frame = match resolved.frame_id.as_deref() {
        Some(frame_id) => Some(frame_index_path(client, top_session_id, frame_id).await),
        None => None,
    };
    ElementCapture {
        probe_failed: probe.is_err(),
        probe: probe.ok(),
        frame_probe_failed: frame.as_ref().is_some_and(Result::is_err),
        frame: frame.and_then(Result::ok),
        position: None,
        scroll_delta: None,
        scroll_position: None,
        state_changed: None,
    }
}

pub async fn probe_url(client: &CdpClient, session_id: &str) -> Result<String, String> {
    let result: EvaluateResult = client
        .send_command_typed(
            "Runtime.evaluate",
            &crate::native::cdp::types::EvaluateParams {
                expression: "location.href".to_string(),
                return_by_value: Some(true),
                await_promise: Some(false),
            },
            Some(session_id),
        )
        .await?;
    result
        .result
        .value
        .and_then(|value| value.as_str().map(str::to_string))
        .ok_or_else(|| "Could not read page URL for codegen".to_string())
}

pub async fn frame_index_path(
    client: &CdpClient,
    session_id: &str,
    frame_id: &str,
) -> Result<Vec<usize>, String> {
    let tree = client
        .send_command_no_params("Page.getFrameTree", Some(session_id))
        .await?;
    let root = tree
        .get("frameTree")
        .ok_or("Could not read frame tree for codegen")?;
    if let Some(path) = frame_index_path_in_tree(root, frame_id) {
        return Ok(path);
    }
    frame_owner_index_path(client, session_id, frame_id).await
}

async fn frame_owner_index_path(
    client: &CdpClient,
    session_id: &str,
    frame_id: &str,
) -> Result<Vec<usize>, String> {
    let owner = client
        .send_command(
            "DOM.getFrameOwner",
            Some(serde_json::json!({ "frameId": frame_id })),
            Some(session_id),
        )
        .await?;
    let backend_node_id = owner
        .get("backendNodeId")
        .and_then(serde_json::Value::as_i64)
        .ok_or("Frame owner has no backend node ID")?;
    let resolved: DomResolveNodeResult = client
        .send_command_typed(
            "DOM.resolveNode",
            &DomResolveNodeParams {
                backend_node_id: Some(backend_node_id),
                node_id: None,
                object_group: Some("agent-browser-codegen".to_string()),
            },
            Some(session_id),
        )
        .await?;
    let object_id = resolved
        .object
        .object_id
        .ok_or("Could not resolve the frame owner")?;
    let result: EvaluateResult = client
        .send_command_typed(
            "Runtime.callFunctionOn",
            &CallFunctionOnParams {
                function_declaration: r#"function() {
                    const path = [];
                    let frame = this;
                    while (frame) {
                        const frames = Array.from(frame.ownerDocument.querySelectorAll('iframe, frame'));
                        const index = frames.indexOf(frame);
                        if (index < 0) throw new Error('Frame owner is not in DOM frame order');
                        path.unshift(index);
                        frame = frame.ownerDocument.defaultView.frameElement;
                    }
                    return path;
                }"#
                .to_string(),
                object_id: Some(object_id),
                arguments: None,
                return_by_value: Some(true),
                await_promise: Some(false),
            },
            Some(session_id),
        )
        .await?;
    if let Some(details) = result.exception_details {
        return Err(format!(
            "Could not calculate frame owner path: {}",
            details.text
        ));
    }
    serde_json::from_value(result.result.value.unwrap_or_default())
        .map_err(|error| format!("Could not decode frame owner path: {error}"))
}

fn frame_index_path_in_tree(tree: &serde_json::Value, wanted: &str) -> Option<Vec<usize>> {
    fn visit(tree: &serde_json::Value, wanted: &str, path: &mut Vec<usize>) -> bool {
        if tree
            .get("frame")
            .and_then(|frame| frame.get("id"))
            .and_then(|id| id.as_str())
            == Some(wanted)
        {
            return true;
        }
        if let Some(children) = tree.get("childFrames").and_then(|value| value.as_array()) {
            for (index, child) in children.iter().enumerate() {
                path.push(index);
                if visit(child, wanted, path) {
                    return true;
                }
                path.pop();
            }
        }
        false
    }
    let mut path = Vec::new();
    if visit(tree, wanted, &mut path) {
        Some(path)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{frame_index_path_in_tree, probe_from_value};
    use serde_json::json;

    #[test]
    fn decodes_exact_probe_result_without_inventing_selector_data() {
        let probe = probe_from_value(&json!({
            "selector": "button:nth-of-type(2)",
            "testId": null,
            "href": "https://example.com",
            "type": "password"
        }));

        assert_eq!(probe.selector.as_deref(), Some("button:nth-of-type(2)"));
        assert_eq!(probe.test_id, None);
        assert_eq!(probe.input_type.as_deref(), Some("password"));
    }

    #[test]
    fn frame_index_path_tracks_nested_iframes() {
        let tree = json!({
            "frame": { "id": "root" },
            "childFrames": [
                { "frame": { "id": "first" } },
                { "frame": { "id": "second" }, "childFrames": [
                    { "frame": { "id": "nested" } }
                ] }
            ]
        });
        assert_eq!(frame_index_path_in_tree(&tree, "root"), Some(vec![]));
        assert_eq!(frame_index_path_in_tree(&tree, "first"), Some(vec![0]));
        assert_eq!(frame_index_path_in_tree(&tree, "nested"), Some(vec![1, 0]));
        assert_eq!(frame_index_path_in_tree(&tree, "missing"), None);
    }
}
