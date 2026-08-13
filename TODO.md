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

- [ ] Render Recorder and Playwright from typed selectors and typed actions instead of shared flattened strings
- [ ] Emit only non-empty schema-valid Recorder selectors
- [ ] Omit Recorder multi-select, upload, type, explicit page creation, and unsupported waits with stable format warnings
- [ ] Emit back, forward, and reload as observed Recorder navigation only when a final URL exists, with a lossy warning
- [ ] Emit Recorder page targets from current URL and warn when same-URL pages are ambiguous
- [ ] Emit full Playwright multi-value `selectOption`, `setInputFiles`, `pressSequentially`, clear mode, delay, normalized chords, pointer input, page and element scroll, page scope, viewport scope, and scoped close
- [ ] Use unique Playwright variables for all logical pages and popups
- [ ] Render CSS and XPath correctly for Playwright
- [ ] Replace manual quote escaping with real JavaScript string serialization
- [ ] Render initial context viewport, device scale, mobile, and touch options; warn on unsupported later mode changes
- [ ] Prove `frameLocator('iframe, frame').nth(index)` mapping with `<iframe>`, `<frame>`, nested-frame, and OOPIF fixtures; omit and warn when mapping is not safe
- [ ] Return structured grouped warnings with stable code, count, up to ten action IDs, affected format, and format-specific emitted step number when applicable
- [ ] Return captured action, internal step, emitted, omitted, lossy, capture warning, security warning, and cleanup warning counts
- [ ] Add universal typed-value and stronger confirmed-password warnings without including captured values
- [ ] Pin exact root development dependencies `@puppeteer/replay` 4.0.2 and `@playwright/test` 1.62.1 with pnpm
- [ ] Add shared golden artifacts under `cli/src/native/codegen/test-fixtures/`
- [ ] Add Rust golden comparisons against production renderer output
- [ ] Add a root Node conformance test that parses the same Recorder fixture and runs Playwright `--list` on the same generated spec
- [ ] Add `test:codegen-formats` and a separate Node CI job with frozen pnpm install
- [ ] Cover every supported step, hostile strings and line terminators, two popups, same-URL tabs, nested frames, mobile context, multi-select, upload, chords, scoped navigation, scoped viewport, scoped close, omissions, and loss warnings in shared fixtures
- [ ] Update format support and security documentation in the same change
- [ ] Commit: `fix(codegen): generate schema-valid faithful test artifacts`

## Phase 5: Documentation and end-to-end verification

- [ ] Audit `cli/src/output.rs` help, examples, warnings, and status formatting
- [ ] Audit `README.md` command and feature documentation
- [ ] Audit `skill-data/core/SKILL.md`, `skill-data/core/references/codegen.md`, and `skill-data/core/references/commands.md`
- [ ] Audit `docs/src/app/codegen/page.mdx`, `docs/src/app/commands/page.mdx`, and `docs/src/app/security/page.mdx`
- [ ] Audit MCP tool names, descriptions, schemas, argument conversion, and parity tests
- [ ] Audit parser usage and error text and all affected inline source comments
- [ ] Confirm that `skills/agent-browser/SKILL.md` has no feature content
- [ ] Add or update ignored Chrome e2e tests for exact selectors, CSS password warning, selector omission, navigation assertion, same-URL tabs, popups, frames, browser relaunch, recovery, artifact writing, cleanup, and discard
- [ ] Run `cargo test --manifest-path cli/Cargo.toml`
- [ ] Run `cargo fmt --manifest-path cli/Cargo.toml -- --check`
- [ ] Run `cargo clippy --manifest-path cli/Cargo.toml -- -D warnings`
- [ ] Run `pnpm test:codegen-formats`
- [ ] Run `pnpm --dir docs lint`
- [ ] Run `pnpm --dir docs build`
- [ ] Run `cargo test --manifest-path cli/Cargo.toml e2e -- --ignored --test-threads=1`
- [ ] Record codegen-active timing evidence and actual CDP command counts
- [ ] Confirm that no dashboard or changelog changes were added
- [ ] Commit: `docs(codegen): document durable flow generation`

## Review

- [ ] Code reviewed against every confirmed design decision in `PLAN.md`
- [ ] CLI and MCP behavior remain aligned
- [ ] All changed user-facing behavior is documented in the same phase
- [ ] No warning message exposes typed, selected, uploaded, or credential values
- [ ] No output format receives an empty, transient, guessed, or wrong-page target
- [ ] No tombstone tests were added only to prove that removed behavior stays absent
- [ ] Each phase commit is clean and uses the exact planned message
- [ ] All TODO items are checked
