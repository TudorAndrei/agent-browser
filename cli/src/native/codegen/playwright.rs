use super::{ClickKind, Scope, Step, Target};
use std::collections::HashMap;

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn locator(target: &Target, page: &str, frame: &[usize]) -> String {
    let mut page = page.to_string();
    for index in frame {
        page.push_str(&format!(".frameLocator('iframe').nth({index})"));
    }
    if let Some(test_id) = &target.test_id {
        return format!("{page}.getByTestId({})", quote(test_id));
    }
    if let (Some(role), Some(name)) = (&target.role, &target.name) {
        let mut result = format!(
            "{page}.getByRole({}, {{ name: {} }})",
            quote(role),
            quote(name)
        );
        if let Some(nth) = target.nth {
            result.push_str(&format!(".nth({nth})"));
        }
        return result;
    }
    let selector = target
        .selectors
        .first()
        .and_then(|alternative| alternative.first())
        .cloned()
        .unwrap_or_else(|| "body".to_string());
    format!("{page}.locator({})", quote(&selector))
}

fn page_for(
    scope: &Scope,
    pages: &mut HashMap<String, String>,
    lines: &mut Vec<String>,
    next_page: &mut usize,
) -> String {
    if scope.target == "main" {
        return "page".to_string();
    }
    if let Some(page) = pages.get(&scope.target) {
        return page.clone();
    }
    // A click-created popup has no stable URL until the following action. Bind
    // that next non-main scope to the popup instead of manufacturing a page.
    if let Some(popup) = pages.remove("__pending_popup") {
        pages.insert(scope.target.clone(), popup.clone());
        return popup;
    }
    *next_page += 1;
    let page = format!("page{next_page}");
    lines.push(format!("  const {page} = await context.newPage();"));
    lines.push(format!("  await {page}.goto({});", quote(&scope.target)));
    pages.insert(scope.target.clone(), page.clone());
    page
}

pub fn render_playwright(title: &str, steps: &[Step]) -> String {
    let mut lines = vec![
        "import { test, expect } from '@playwright/test';".to_string(),
        String::new(),
        format!("test({}, async ({{ page, context }}) => {{", quote(title)),
    ];
    let mut pages = HashMap::new();
    let mut next_page = 1;
    for step in steps {
        match step {
            Step::SetViewport { width, height, .. } => lines.push(format!(
                "  await page.setViewportSize({{ width: {width}, height: {height} }});"
            )),
            Step::Navigate { url } => lines.push(format!("  await page.goto({});", quote(url))),
            Step::Click {
                target,
                count,
                kind,
                opens_popup,
                scope,
                asserted_url,
                ..
            } => {
                let op = match kind {
                    ClickKind::Check => "check()".to_string(),
                    ClickKind::Uncheck => "uncheck()".to_string(),
                    _ if *count == 2 => "dblclick()".to_string(),
                    _ => "click()".to_string(),
                };
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                if *opens_popup {
                    lines.push(format!(
                        "  const popupPromise = {page}.waitForEvent('popup');"
                    ));
                }
                lines.push(format!(
                    "  await {}.{};",
                    locator(target, &page, &scope.frame),
                    op
                ));
                if *opens_popup {
                    lines.push("  const popup = await popupPromise;".to_string());
                    pages.insert("__pending_popup".to_string(), "popup".to_string());
                }
                if let Some(url) = asserted_url {
                    let asserted_page = if *opens_popup { "popup" } else { &page };
                    lines.push(format!(
                        "  await expect({asserted_page}).toHaveURL({});",
                        quote(url)
                    ));
                }
            }
            Step::Hover { target, scope } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!(
                    "  await {}.hover();",
                    locator(target, &page, &scope.frame)
                ))
            }
            Step::Change {
                target,
                value,
                is_select,
                scope,
                asserted_url,
                ..
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!(
                    "  await {}.{}({});",
                    locator(target, &page, &scope.frame),
                    if *is_select { "selectOption" } else { "fill" },
                    quote(value)
                ));
                if let Some(url) = asserted_url {
                    lines.push(format!("  await expect({page}).toHaveURL({});", quote(url)));
                }
            }
            Step::KeyDown { key, scope } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!("  await {page}.keyboard.down({});", quote(key)))
            }
            Step::KeyUp { key, scope } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!("  await {page}.keyboard.up({});", quote(key)))
            }
            Step::Scroll { x, y, scope, .. } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!("  await {page}.mouse.wheel({x}, {y});"))
            }
            Step::WaitForElement {
                target,
                visible,
                properties,
                count,
                scope,
                ..
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let loc = locator(target, &page, &scope.frame);
                if let Some(visible) = visible {
                    lines.push(if *visible {
                        format!("  await expect({loc}).toBeVisible();")
                    } else {
                        format!("  await expect({loc}).toBeHidden();")
                    });
                }
                if let Some(checked) = properties.get("checked").and_then(|v| v.as_bool()) {
                    lines.push(if checked {
                        format!("  await expect({loc}).toBeChecked();")
                    } else {
                        format!("  await expect({loc}).not.toBeChecked();")
                    });
                }
                if let Some(disabled) = properties.get("disabled").and_then(|v| v.as_bool()) {
                    lines.push(if disabled {
                        format!("  await expect({loc}).toBeDisabled();")
                    } else {
                        format!("  await expect({loc}).toBeEnabled();")
                    });
                }
                if let Some(count) = count {
                    lines.push(format!("  await expect({loc}).toHaveCount({count});"));
                }
            }
            Step::Close => lines.push("  await page.close();".to_string()),
            Step::NewTab { url } => {
                next_page += 1;
                let page = format!("page{next_page}");
                lines.push(format!("  const {page} = await context.newPage();"));
                if let Some(url) = url {
                    lines.push(format!("  await {page}.goto({});", quote(url)));
                    pages.insert(url.clone(), page);
                }
            }
        }
    }
    lines.push("});".to_string());
    lines.push(String::new());
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native::codegen::{ClickKind, Scope, Step, Target};
    #[test]
    fn escapes_locator_strings() {
        let target = Target {
            selectors: vec![vec!["#x".into()]],
            role: Some("button".into()),
            name: Some("O'Reilly".into()),
            nth: None,
            test_id: None,
            input_type: None,
        };
        let rendered = render_playwright(
            "a\\b",
            &[Step::Click {
                target,
                count: 1,
                kind: ClickKind::Click,
                opens_popup: false,
                scope: Scope::default(),
                asserted_url: None,
            }],
        );
        assert!(rendered.contains("O\\'Reilly"));
        assert!(rendered.contains("a\\\\b"));
    }

    #[test]
    fn renders_popup_tab_and_frame_scopes() {
        let target = || Target {
            selectors: vec![vec!["#pay".into()]],
            role: None,
            name: None,
            nth: None,
            test_id: None,
            input_type: None,
        };
        let rendered = render_playwright(
            "flow",
            &[
                Step::Click {
                    target: target(),
                    count: 1,
                    kind: ClickKind::Click,
                    opens_popup: true,
                    scope: Scope::default(),
                    asserted_url: None,
                },
                Step::NewTab {
                    url: Some("https://other.example".into()),
                },
                Step::Click {
                    target: target(),
                    count: 1,
                    kind: ClickKind::Click,
                    opens_popup: false,
                    scope: Scope {
                        target: "https://other.example".into(),
                        frame: vec![0],
                    },
                    asserted_url: None,
                },
            ],
        );
        assert!(rendered.contains("waitForEvent('popup')"));
        assert!(rendered.contains("context.newPage()"));
        assert!(rendered.contains("frameLocator('iframe').nth(0)"));
    }

    #[test]
    fn renders_all_locator_preferences_and_assertions() {
        let target = |test_id: Option<&str>, role: Option<&str>, name: Option<&str>| Target {
            selectors: vec![vec![".fallback".into()]],
            role: role.map(str::to_string),
            name: name.map(str::to_string),
            nth: Some(1),
            test_id: test_id.map(str::to_string),
            input_type: None,
        };
        let rendered = render_playwright(
            "assertions",
            &[
                Step::Click {
                    target: target(Some("save"), Some("button"), Some("Save")),
                    count: 1,
                    kind: ClickKind::Check,
                    opens_popup: false,
                    scope: Scope::default(),
                    asserted_url: Some("https://example.com/done".into()),
                },
                Step::Click {
                    target: target(None, Some("button"), Some("Cancel")),
                    count: 1,
                    kind: ClickKind::Uncheck,
                    opens_popup: false,
                    scope: Scope::default(),
                    asserted_url: None,
                },
                Step::WaitForElement {
                    target: target(None, None, None),
                    scope: Scope::default(),
                    visible: Some(false),
                    properties: serde_json::json!({ "checked": false, "disabled": true })
                        .as_object()
                        .unwrap()
                        .clone(),
                    count: Some(2),
                    operator: Some("==".into()),
                },
            ],
        );
        assert!(rendered.contains("getByTestId('save').check()"));
        assert!(rendered.contains("getByRole('button', { name: 'Cancel' }).nth(1).uncheck()"));
        assert!(rendered.contains("locator('.fallback')).toBeHidden()"));
        assert!(rendered.contains("locator('.fallback')).not.toBeChecked()"));
        assert!(rendered.contains("locator('.fallback')).toBeDisabled()"));
        assert!(rendered.contains("locator('.fallback')).toHaveCount(2)"));
        assert!(rendered.contains("expect(page).toHaveURL('https://example.com/done')"));
    }
}
