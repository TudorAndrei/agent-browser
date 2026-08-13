use super::{ClickKind, NavigationKind, PointerKind, Scope, SelectorKind, Step, Target};
use std::collections::HashMap;

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn locator(target: &Target, page: &str, frame: &[usize]) -> Option<String> {
    let mut page = page.to_string();
    for index in frame {
        page.push_str(&format!(".frameLocator('iframe, frame').nth({index})"));
    }
    target.selectors.first().map(|selector| match selector {
        SelectorKind::TestId { value } => {
            format!("{page}.getByTestId({})", quote(value))
        }
        SelectorKind::Role { role, name, nth } => {
            let mut result = format!(
                "{page}.getByRole({}, {{ name: {} }})",
                quote(role),
                quote(name)
            );
            if let Some(nth) = nth {
                result.push_str(&format!(".nth({nth})"));
            }
            result
        }
        SelectorKind::Css { value } => {
            format!("{page}.locator({})", quote(value))
        }
        SelectorKind::XPath { value } => {
            format!("{page}.locator({})", quote(&format!("xpath={value}")))
        }
    })
}

fn page_for(
    scope: &Scope,
    pages: &mut HashMap<String, String>,
    lines: &mut Vec<String>,
    next_page: &mut usize,
) -> String {
    if scope.target == "main" || scope.target == "p1" {
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
    let mut next_popup = 0usize;
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
                let Some(locator) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                if *opens_popup {
                    next_popup += 1;
                    lines.push(format!(
                        "  const popupPromise{next_popup} = {page}.waitForEvent('popup');"
                    ));
                }
                lines.push(format!("  await {locator}.{op};"));
                if *opens_popup {
                    lines.push(format!(
                        "  const popup{next_popup} = await popupPromise{next_popup};"
                    ));
                    pages.insert("__pending_popup".to_string(), format!("popup{next_popup}"));
                }
                if let Some(url) = asserted_url {
                    let popup_page = format!("popup{next_popup}");
                    let asserted_page = if *opens_popup { &popup_page } else { &page };
                    lines.push(format!(
                        "  await expect({asserted_page}).toHaveURL({});",
                        quote(url)
                    ));
                }
            }
            Step::Hover { target, scope } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let Some(locator) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                lines.push(format!("  await {locator}.hover();"))
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
                let Some(locator) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                lines.push(format!(
                    "  await {locator}.{}({});",
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
                let Some(loc) = locator(target, &page, &scope.frame) else {
                    continue;
                };
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
            Step::ScopedViewport {
                width,
                height,
                scope,
                ..
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!(
                    "  await {page}.setViewportSize({{ width: {width}, height: {height} }});"
                ));
            }
            Step::ScopedNavigation {
                kind, url, scope, ..
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(match kind {
                    NavigationKind::Goto => format!("  await {page}.goto({});", quote(url)),
                    NavigationKind::Back => format!("  await {page}.goBack();"),
                    NavigationKind::Forward => format!("  await {page}.goForward();"),
                    NavigationKind::Reload => format!("  await {page}.reload();"),
                });
            }
            Step::Pointer {
                target,
                kind,
                pointer,
                button,
                count,
                position,
                opens_popup,
                popup_page,
                scope,
                asserted_url,
                ..
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                if *opens_popup {
                    next_popup += 1;
                    lines.push(format!(
                        "  const popupPromise{next_popup} = {page}.waitForEvent('popup');"
                    ));
                }
                let Some(loc) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                let point = *position;
                let position_options = position
                    .map(|(x, y)| format!(", position: {{ x: {x}, y: {y} }}"))
                    .unwrap_or_default();
                let operation = match (pointer, kind) {
                    (PointerKind::Touch, _) => {
                        let (x, y) = point.unwrap_or((0.0, 0.0));
                        format!("tap({{ position: {{ x: {x}, y: {y} }} }})")
                    }
                    (_, ClickKind::Check) => "check()".to_string(),
                    (_, ClickKind::Uncheck) => "uncheck()".to_string(),
                    (_, _) if *count == 2 => {
                        format!(
                            "dblclick({{ button: {}{position_options} }})",
                            quote(button)
                        )
                    }
                    _ => format!("click({{ button: {}{position_options} }})", quote(button)),
                };
                lines.push(format!("  await {loc}.{operation};"));
                if *opens_popup {
                    lines.push(format!(
                        "  const popup{next_popup} = await popupPromise{next_popup};"
                    ));
                    let popup = format!("popup{next_popup}");
                    if let Some(page_id) = popup_page {
                        pages.insert(page_id.clone(), popup);
                    } else {
                        pages.insert("__pending_popup".to_string(), popup);
                    }
                }
                if let Some(url) = asserted_url {
                    let asserted_page = if *opens_popup {
                        format!("popup{next_popup}")
                    } else {
                        page
                    };
                    lines.push(format!(
                        "  await expect({asserted_page}).toHaveURL({});",
                        quote(url)
                    ));
                }
            }
            Step::Fill {
                target,
                value,
                scope,
                asserted_url,
            }
            | Step::SetValue {
                target,
                value,
                scope,
                asserted_url,
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let Some(locator) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                lines.push(format!("  await {locator}.fill({});", quote(value)));
                if let Some(url) = asserted_url {
                    lines.push(format!("  await expect({page}).toHaveURL({});", quote(url)));
                }
            }
            Step::Type {
                target,
                text,
                clear,
                delay_ms,
                scope,
                asserted_url,
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let Some(loc) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                if *clear {
                    lines.push(format!("  await {loc}.fill('');"));
                }
                let options = delay_ms
                    .map(|delay| format!(", {{ delay: {delay} }}"))
                    .unwrap_or_default();
                lines.push(format!(
                    "  await {loc}.pressSequentially({}{});",
                    quote(text),
                    options
                ));
                if let Some(url) = asserted_url {
                    lines.push(format!("  await expect({page}).toHaveURL({});", quote(url)));
                }
            }
            Step::Select {
                target,
                values,
                scope,
                asserted_url,
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let Some(locator) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                let values = serde_json::to_string(values).unwrap_or_else(|_| "[]".to_string());
                lines.push(format!("  await {locator}.selectOption({values});"));
                if let Some(url) = asserted_url {
                    lines.push(format!("  await expect({page}).toHaveURL({});", quote(url)));
                }
            }
            Step::Press {
                modifiers,
                key,
                scope,
                asserted_url,
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let chord = modifiers
                    .iter()
                    .chain(std::iter::once(key))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("+");
                lines.push(format!("  await {page}.keyboard.press({});", quote(&chord)));
                if let Some(url) = asserted_url {
                    lines.push(format!("  await expect({page}).toHaveURL({});", quote(url)));
                }
            }
            Step::Wheel {
                target,
                x,
                y,
                scope,
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                if let Some(target) = target {
                    let Some(locator) = locator(target, &page, &scope.frame) else {
                        continue;
                    };
                    lines.push(format!(
                        "  await {locator}.evaluate((element, delta) => element.scrollBy(delta.x, delta.y), {{ x: {x}, y: {y} }});"
                    ));
                } else {
                    lines.push(format!("  await {page}.mouse.wheel({x}, {y});"));
                }
            }
            Step::Upload {
                target,
                paths,
                scope,
            } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                let Some(locator) = locator(target, &page, &scope.frame) else {
                    continue;
                };
                let paths = serde_json::to_string(paths).unwrap_or_else(|_| "[]".to_string());
                lines.push(format!("  await {locator}.setInputFiles({paths});"));
            }
            Step::NewPage { url, scope } => {
                next_page += 1;
                let page = format!("page{next_page}");
                lines.push(format!("  const {page} = await context.newPage();"));
                if let Some(url) = url {
                    lines.push(format!("  await {page}.goto({});", quote(url)));
                }
                pages.insert(scope.target.clone(), page);
            }
            Step::OpenPage { url, scope, .. } => {
                next_page += 1;
                let page = format!("page{next_page}");
                lines.push(format!("  const {page} = await context.newPage();"));
                lines.push(format!("  await {page}.goto({});", quote(url)));
                pages.insert(scope.target.clone(), page);
            }
            Step::ClosePage { scope } => {
                let page = page_for(scope, &mut pages, &mut lines, &mut next_page);
                lines.push(format!("  await {page}.close();"));
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
    use crate::native::codegen::{ClickKind, Scope, SelectorKind, Step, Target};
    #[test]
    fn escapes_locator_strings() {
        let target = Target {
            selectors: vec![SelectorKind::Role {
                role: "button".into(),
                name: "O'Reilly".into(),
                nth: None,
            }],
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
            selectors: vec![SelectorKind::Css {
                value: "#pay".into(),
            }],
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
        assert!(rendered.contains("frameLocator('iframe, frame').nth(0)"));
    }

    #[test]
    fn renders_all_locator_preferences_and_assertions() {
        let target = |test_id: Option<&str>, role: Option<&str>, name: Option<&str>| Target {
            selectors: test_id
                .map(|value| SelectorKind::TestId {
                    value: value.to_string(),
                })
                .into_iter()
                .chain(role.zip(name).map(|(role, name)| SelectorKind::Role {
                    role: role.to_string(),
                    name: name.to_string(),
                    nth: Some(1),
                }))
                .chain(std::iter::once(SelectorKind::Css {
                    value: ".fallback".into(),
                }))
                .collect(),
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

    #[test]
    fn renders_typed_input_actions_without_flattening_values() {
        let target = Target {
            selectors: vec![SelectorKind::Css {
                value: "#target".into(),
            }],
            input_type: None,
        };
        let rendered = render_playwright(
            "typed",
            &[
                Step::Select {
                    target: target.clone(),
                    values: vec!["a".into(), "b".into()],
                    scope: Scope::default(),
                    asserted_url: None,
                },
                Step::Type {
                    target: target.clone(),
                    text: "hello".into(),
                    clear: true,
                    delay_ms: Some(20),
                    scope: Scope::default(),
                    asserted_url: None,
                },
                Step::Press {
                    modifiers: vec!["Control".into(), "Shift".into()],
                    key: "a".into(),
                    scope: Scope::default(),
                    asserted_url: Some("https://example.com/done".into()),
                },
                Step::Upload {
                    target,
                    paths: vec!["a.txt".into(), "b.txt".into()],
                    scope: Scope::default(),
                },
            ],
        );

        assert!(rendered.contains("selectOption([\"a\",\"b\"])"));
        assert!(rendered.contains("fill('')"));
        assert!(rendered.contains("pressSequentially('hello', { delay: 20 })"));
        assert!(rendered.contains("keyboard.press('Control+Shift+a')"));
        assert!(rendered.contains("setInputFiles([\"a.txt\",\"b.txt\"])"));
        assert!(rendered.contains("expect(page).toHaveURL('https://example.com/done')"));
    }

    #[test]
    fn omits_an_action_without_a_safe_selector() {
        let rendered = render_playwright(
            "unsafe",
            &[Step::Fill {
                target: Target {
                    selectors: Vec::new(),
                    input_type: None,
                },
                value: "value".into(),
                scope: Scope::default(),
                asserted_url: None,
            }],
        );

        assert!(!rendered.contains(".fill("));
        assert!(!rendered.contains("locator('body')"));
    }
}
