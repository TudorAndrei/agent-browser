# Codegen

`agent-browser codegen` turns successful browser actions into a reusable Chrome DevTools Recorder flow. Start capture before performing actions, then stop it to print or save the result.

```bash
agent-browser codegen start --title "login flow"
agent-browser open https://example.com/login
agent-browser fill "#email" "a@example.com"
agent-browser click "#submit"
agent-browser isvisible "#welcome"
agent-browser codegen stop ./login.flow.json
```

The default output is JSON compatible with the Chrome DevTools Recorder and `@puppeteer/replay`. Popup targets and iframe paths are retained in Recorder JSON and Playwright output. To write a Playwright test instead, use `agent-browser codegen stop ./login.spec.ts --format playwright`.

`snapshot`, screenshots, page reads, and other observation commands do not become flow steps. The command captures typed values verbatim, including password values. The daemon restores an unfinished flow from its owner-only sidecar and capture metadata after a restart. Treat both files and the generated artifact as sensitive material and do not commit credentials.
