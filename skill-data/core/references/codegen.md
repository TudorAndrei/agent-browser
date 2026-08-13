# Codegen

`agent-browser codegen` turns supported successful browser actions into a reusable Chrome DevTools Recorder flow. Start capture before performing actions, then stop it to print or save the result.

```bash
agent-browser codegen start --title "login flow"
agent-browser open https://example.com/login
agent-browser fill "#email" "a@example.com"
agent-browser click "#submit"
agent-browser isvisible "#welcome"
agent-browser codegen stop ./login.flow.json
```

The default output is JSON for Chrome DevTools Recorder and `@puppeteer/replay`. To write a Playwright test instead, use `agent-browser codegen stop ./login.spec.ts --format playwright`.

Direct CSS selectors, `xpath=` selectors, snapshot refs with accessible names, and uniquely probed test IDs can become safe targets. Codegen omits an action when it cannot produce a safe target. Direct `text=` selectors, bare XPath, semantic locator marker actions, and most wait variants are not recorded. `snapshot`, screenshots, page reads, and other observation commands do not become flow steps. Other successful mutating or wait commands produce omission warnings.

The command captures typed values verbatim, including password values, selected values, and upload paths. The daemon restores an unfinished flow from its owner-only append-only journal after a restart. `agent-browser codegen status` reports active, restored, degraded, recovery-error, and cleanup-pending states. It also reports capture and omission warnings. Use `agent-browser codegen discard` to delete an unfinished or damaged flow. A successful stop removes the journal. Treat the journal and generated artifact as sensitive material and do not commit credentials.

Recorder JSON omits sequential typing, multi-value select, upload, key chords, and explicit page creation because its schema cannot keep the exact action intent. It converts back, forward, and reload to navigation to the observed final URL and reports a lossy warning. Recorder identifies a non-main page by URL, so same-URL pages are ambiguous. Playwright keeps typed selector kinds, multi-value select, upload paths, typing clear mode and delay, modifier chords, pointer input, element and page scroll, logical page identity, scoped close, and initial mobile context options. `codegen stop` reports internal, emitted, omitted, and lossy step counts and grouped warnings for the selected format.
