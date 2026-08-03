use crate::native::cdp::client::CdpClient;
use crate::native::cdp::types::{
    CallFunctionOnParams, DomResolveNodeParams, DomResolveNodeResult, EvaluateResult,
};

#[derive(Clone, Debug, Default)]
pub struct Probe {
    pub selector: Option<String>,
    pub test_id: Option<String>,
    pub href: Option<String>,
    pub input_type: Option<String>,
}

const ELEMENT_PROBE: &str = r#"function() {
  const esc = (value) => CSS.escape(value);
  const unique = (selector) => { try { return document.querySelectorAll(selector).length === 1; } catch { return false; } };
  const selector = (() => {
    const testId = this.getAttribute('data-testid');
    if (testId) return `[data-testid="${esc(testId)}"]`;
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
  return { selector, testId: this.getAttribute('data-testid') || null, href: location.href, type: this instanceof HTMLInputElement ? this.type : null };
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
    let result: EvaluateResult = client
        .send_command_typed(
            "Runtime.callFunctionOn",
            &CallFunctionOnParams {
                function_declaration: ELEMENT_PROBE.to_string(),
                object_id: Some(object_id),
                arguments: None,
                return_by_value: Some(true),
                await_promise: Some(false),
            },
            Some(session_id),
        )
        .await?;
    let value = result.result.value.unwrap_or_default();
    Ok(Probe {
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
    })
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
    frame_index_path_in_tree(root, frame_id)
        .ok_or_else(|| "Active frame is not in the page frame tree".to_string())
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
    use super::{frame_index_path_in_tree, ELEMENT_PROBE};
    use serde_json::json;

    #[test]
    fn probe_prefers_test_id_then_id_then_positional_selector() {
        let test_id = ELEMENT_PROBE.find("data-testid").unwrap();
        let id = ELEMENT_PROBE.find("this.id").unwrap();
        let positional = ELEMENT_PROBE.find("nth-of-type").unwrap();
        assert!(test_id < id);
        assert!(id < positional);
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
