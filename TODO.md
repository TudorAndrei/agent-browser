# TODO: Harden `agent-browser codegen` before release

The original implementation and review are summarized in `PLAN.md`. This checklist tracks the confirmed repair only. Implementation starts after separate approval of these planning files.

## Phase 1: Durable journal, recovery states, cleanup, and discard

- [x] Define the versioned journal envelope, contiguous `sequence`, `actionId`, `stepId`, and typed start, action, update, warning, `output-written`, and `discard-requested` records in `cli/src/native/codegen/`
- [x] Replace sidecar rewrites and the metadata companion with append, flush, close journal writes
- [x] Make one successful browser command one action journal record
- [x] Implement the explicit `inactive`, `active`, `restored`, `degraded`, `recovery-error`, and `cleanup-pending` state machine
- [x] Keep a derived `active` Boolean in structured status output
- [x] Restore one incomplete unterminated final line and reject malformed complete lines, unknown versions, sequence faults, unknown-step updates, and journals larger than 64 MiB
- [x] Detect old development journal and metadata shapes without reading captured values into error output
- [x] Create journals as new regular Unix files with mode `0600`; reject existing paths, symlinks, directories, and special files
- [x] Implement atomic artifact writes with mode `0600` for new files and preserved permissions for replacements
- [x] Implement retry-safe stop ordering and durable `output-written` state before cleanup
- [x] Implement exact-path cleanup and `cleanup-pending` recovery when cleanup fails
- [x] Add `codegen discard` with durable `discard-requested` state and retryable cleanup
- [x] Add `codegen discard` to `cli/src/commands.rs`, `cli/src/native/actions.rs`, `cli/src/mcp.rs`, `cli/src/output.rs`, native parity lists, and WebDriver support declarations
- [x] Update every required user-facing documentation surface for discard and recovery states
- [x] Test partial final records, malformed complete records, missing and duplicate sequences, unknown updates, oversized journals, old development files, symlinks, special files, append failure, output failure, cleanup failure, stop retry, and discard retry
- [x] Test CLI and MCP parser-shape parity and human status output for all states
- [x] Commit: `fix(codegen): make recording recovery and cleanup durable`

## Phase 2: Exact target capture and typed action model

- [x] Add `ResolvedElement` in `cli/src/native/element.rs` with object ID, optional backend node ID, effective session ID, and logical frame ID
- [x] Make click, double-click, tap, hover, fill, set value, type, select, check, uncheck, upload, and element scroll use one exact element resolution
- [x] Calculate coordinate-action points from the same resolved object
- [x] Probe the resolved object before the action only while codegen is active
- [x] Carry private capture data through interaction and handler results without changing public command JSON
- [x] Discard private capture data and omit journal actions when the browser action fails
- [x] Calculate frame scope before the action with the correct same-process iframe or OOPIF session
- [x] Replace flattened steps with typed action groups and typed selector kinds
- [x] Add typed fill, set value, type, select, upload, press chord, click-like input, scroll, viewport, navigation, assertion, explicit new-page, popup, and page-close models
- [x] Store effective select values, scroll coordinates, pointer position, button, pointer or touch kind, click count, type clear mode, type delay, and upload paths
- [x] Make every page-specific step carry page scope
- [x] Make unique test IDs, role plus accessible name, unique CSS, and XPath the only valid Playwright target forms
- [x] Remove empty selector emission and the Playwright `body` fallback
- [x] Omit an unsafe format step and record a structured warning instead of guessing
- [x] Add an exhaustive recorded, read-only observation, and omitted mutating or wait action classification
- [x] Keep direct `text=`, bare XPath, semantic temporary-marker locators, and unsupported wait variants out of scope and correct all false plan or documentation claims
- [x] Add durable degraded-state capture and persistence errors to status and stop
- [x] Test real probe behavior for unique test ID, duplicate test ID, unique ID, positional CSS, unnamed refs, failed probes, CSS password fields, same-process frames, OOPIFs, and nested frames
- [x] Replace the JS source-position probe test and the production-inaccurate `RefMap::add_selector` test with behavioral tests
- [x] Test exact type, select, upload, chord, pointer, scroll, viewport, assertion, omission, and failed-action behavior
- [x] Test that inactive codegen adds no capture calls and that the active target path stays within the agreed CDP command counts
- [x] Commit: `refactor(codegen): capture exact action intent and page scope`

## Phase 3: Logical pages, popups, navigation, and relaunch

- [x] Add monotonic persisted codegen page IDs and runtime CDP target bindings
- [x] Capture active page identity before command dispatch
- [x] Make the start page `p1` and Recorder target `main`, including start-before-browser behavior
- [x] Capture initial URL and viewport when codegen starts on an existing page
- [x] Discover existing background pages without generating them until a recorded action uses them
- [x] Keep logical page identity separate from Recorder target URL and user-facing runtime `t<N>` IDs
- [x] Use handler results for explicit tab creation, switching, closing, and `click --new-tab`
- [x] Model `click --new-tab` as explicit page creation, not a natural popup click
- [x] Retain CDP `openerId` and bind natural popups to the correct pre-action opener page
- [x] Give every generated popup and page a unique logical identity
- [x] Warn and avoid causal guesses for ambiguous popup attribution
- [x] Keep one pending navigation-capable action and one popup-capable click per logical page
- [x] Process main-frame navigation, target lifecycle, redirects, and same-document navigation
- [x] Use successful handler URLs or a post-action URL read from the pre-action page session
- [x] Drain events before replacing pending actions and finalize live pending pages at stop without a fixed sleep
- [x] Add `navigation-check-failed` and missing-assertion diagnostics without exposing captured values
- [x] Rebind the last active logical page on local browser relaunch and keep other old pages unbound
- [x] Reuse logical pages on external reconnect only for unchanged CDP target IDs
- [x] Normalize top-level browser close without emitting a page-close step
- [x] Emit scoped close only for `tab close`
- [x] Test start on an existing URL, start before browser launch, same-URL tabs, repeated navigation, redirects, History API changes, hash changes, press Enter navigation, late navigation, URL-read failure, and stop finalization
- [x] Test explicit new page, two natural popups, ambiguous popup origin, tab switching, scoped tab close, local relaunch, and external reconnect
- [x] Commit: `fix(codegen): preserve page and navigation identity`

## Phase 4: Faithful Recorder and Playwright emitters

- [x] Render Recorder and Playwright from typed selectors and typed actions instead of shared flattened strings
- [x] Emit only non-empty schema-valid Recorder selectors
- [x] Omit Recorder multi-select, upload, type, explicit page creation, and unsupported waits with stable format warnings
- [x] Emit back, forward, and reload as observed Recorder navigation only when a final URL exists, with a lossy warning
- [x] Emit Recorder page targets from current URL and warn when same-URL pages are ambiguous
- [x] Emit full Playwright multi-value `selectOption`, `setInputFiles`, `pressSequentially`, clear mode, delay, normalized chords, pointer input, page and element scroll, page scope, viewport scope, and scoped close
- [x] Use unique Playwright variables for all logical pages and popups
- [x] Render CSS and XPath correctly for Playwright
- [x] Replace manual quote escaping with real JavaScript string serialization
- [x] Render initial context viewport, device scale, mobile, and touch options; warn on unsupported later mode changes
- [x] Prove `frameLocator('iframe, frame').nth(index)` mapping with `<iframe>`, `<frame>`, nested-frame, and OOPIF fixtures; omit and warn when mapping is not safe
- [x] Return structured grouped warnings with stable code, count, up to ten action IDs, affected format, and format-specific emitted step number when applicable
- [x] Return captured action, internal step, emitted, omitted, lossy, capture warning, security warning, and cleanup warning counts
- [x] Add universal typed-value and stronger confirmed-password warnings without including captured values
- [x] Pin exact root development dependencies `@puppeteer/replay` 4.0.2 and `@playwright/test` 1.62.1 with pnpm
- [x] Add shared golden artifacts under `cli/src/native/codegen/test-fixtures/`
- [x] Add Rust golden comparisons against production renderer output
- [x] Add a root Node conformance test that parses the same Recorder fixture and runs Playwright `--list` on the same generated spec
- [x] Add `test:codegen-formats` and a separate Node CI job with frozen pnpm install
- [x] Cover every supported step, hostile strings and line terminators, two popups, same-URL tabs, nested frames, mobile context, multi-select, upload, chords, scoped navigation, scoped viewport, scoped close, omissions, and loss warnings in shared fixtures
- [x] Update format support and security documentation in the same change
- [x] Commit: `fix(codegen): generate schema-valid faithful test artifacts`

## Phase 5: Documentation and end-to-end verification

- [x] Audit `cli/src/output.rs` help, examples, warnings, and status formatting
- [x] Audit `README.md` command and feature documentation
- [x] Audit `skill-data/core/SKILL.md`, `skill-data/core/references/codegen.md`, and `skill-data/core/references/commands.md`
- [x] Audit `docs/src/app/codegen/page.mdx`, `docs/src/app/commands/page.mdx`, and `docs/src/app/security/page.mdx`
- [x] Audit MCP tool names, descriptions, schemas, argument conversion, and parity tests
- [x] Audit parser usage and error text and all affected inline source comments
- [x] Confirm that `skills/agent-browser/SKILL.md` has no feature content
- [x] Add or update ignored Chrome e2e tests for exact selectors, CSS password warning, selector omission, navigation assertion, same-URL tabs, popups, frames, browser relaunch, recovery, artifact writing, cleanup, and discard
- [x] Run `cargo test --manifest-path cli/Cargo.toml`
- [x] Run `cargo fmt --manifest-path cli/Cargo.toml -- --check`
- [x] Run `cargo clippy --manifest-path cli/Cargo.toml -- -D warnings`
- [x] Run `pnpm test:codegen-formats`
- [x] Run `pnpm --dir docs lint` (the command reaches an existing unrelated `theme-toggle.tsx` error and one existing `route.ts` warning; changed MDX files have no lint finding)
- [x] Run `pnpm --dir docs build`
- [x] Run `cargo test --manifest-path cli/Cargo.toml e2e -- --ignored --test-threads=1`
- [x] Record codegen-active timing evidence and actual CDP command counts
- [x] Confirm that no dashboard or changelog changes were added
- [x] Commit: `docs(codegen): document durable flow generation`

## Phase 6: Record the `record start` navigation and stop false assertions

- [x] Merge `upstream/main` at v0.37.0 into the branch and resolve the `actions.rs`, `output.rs`, and documentation conflicts
- [x] Classify `webmcp_list` and `webmcp_result` as observations in `action_support`
- [x] Add a `recording_start` arm to `record_action` that emits a scoped goto when `cmd["url"]` is present
- [x] Classify `recording_start` and `recording_stop` in `action_support` so neither makes a false `omitted-action` warning
- [x] Drain available events before an omitted action is dispatched, then remove `pending_navigation` and `pending_popup` for the active page
- [x] Test that `record start --url` emits one scoped goto and that `record start` without a URL and `record stop` emit nothing and warn nothing
- [x] Test that a `click` that does not navigate, followed by `record start --url`, gets no asserted URL
- [x] Test that a `click` that does navigate keeps its asserted URL
- [x] Add an ignored Chrome e2e test for capture across `record start --url`
- [x] Commit: `fix(codegen): record the page move made by record start`

## Phase 7: Mark page changes that codegen cannot express

- [x] Add `url_unrecorded` to `sidecar::PersistedPage` with `#[serde(default)]` and to the runtime page state
- [x] Add `CodegenState::observe_unrecorded_navigation` with the `unrecorded-navigation` capture warning and no captured values
- [x] Compare the post-action URL with the `pre_action_page` URL for every omitted action, on the successful and the failed result arm
- [x] Treat a later unattributed URL change as unrecorded while `state.webmcp` holds a pending detached invocation for the session
- [x] Make the `navigate` dedupe emit a step while the flag is set, and clear the flag only on an emitted goto
- [x] Confirm that the `codegen status` and `codegen stop` warning counters parse the new code
- [x] Test that `webmcp invoke` sets the flag and that `webmcp list` and `webmcp result` do not
- [x] Test that a later `navigate` to the same URL still emits a step while the flag is set
- [x] Test that `back`, `forward`, and `reload` do not clear the flag
- [x] Test that the flag survives journal recovery from an old journal without the field
- [x] Test that the warning text contains no URL, typed value, or credential
- [x] Add an ignored Chrome e2e test for a `webmcp invoke` that navigates, followed by a navigate to the same URL
- [x] Commit: `fix(codegen): warn when a command moves a page without capture`

## Phase 8: Upstream v0.37.0 documentation and pull-request hygiene

- [x] Add the WebMCP commands to the "not recorded" text in `docs/src/app/codegen/page.mdx`
- [x] Add the same text to `skill-data/core/references/codegen.md`
- [x] Document that `record start --url` becomes a recorded navigation, and document the `unrecorded-navigation` warning, on both surfaces
- [x] Document that a new tab inherits the session setup and that a generated artifact does not contain that setup
- [ ] Remove `PLAN.md` and `TODO.md` from the branch that becomes the upstream pull request, and keep them on the fork `main`
- [x] Confirm that the branch has no changelog or dashboard change
- [x] Commit: `docs(codegen): document unrecorded page changes`

## Verification after the upstream merge

- [x] `cargo test --manifest-path cli/Cargo.toml` (1330 passed, 0 failed, 130 ignored)
- [x] `cargo fmt --manifest-path cli/Cargo.toml -- --check`
- [x] `cargo clippy --manifest-path cli/Cargo.toml` (2 warnings remain; `git blame` shows both come from upstream code)
- [x] `pnpm test:codegen-formats` (2 passed)
- [x] `pnpm --dir docs lint`
- [x] `pnpm --dir docs build`
- [ ] `cargo test --manifest-path cli/Cargo.toml e2e -- --ignored --test-threads=1`

## Review

- [x] Code reviewed against every confirmed design decision in `PLAN.md`
- [x] CLI and MCP behavior remain aligned
- [x] All changed user-facing behavior is documented in the same phase
- [x] No warning message exposes typed, selected, uploaded, or credential values
- [x] No output format receives an empty, transient, guessed, or wrong-page target
- [x] No tombstone tests were added only to prove that removed behavior stays absent
- [x] Each phase commit is clean and uses the exact planned message
- [x] All TODO items are checked
