# Plan: Harden `agent-browser codegen` before release

## Goal

Make codegen safe to release by ensuring that it never creates a valid-looking test that targets the wrong element, loses successful actions without notice, confuses pages, or destroys its recovery data during capture. Preserve exact action intent in an internal model, render only behavior that each output format can represent, and report every omitted or lossy conversion.

## Status and history

The initial codegen implementation exists in commits `74f8406` and `a710ca2`, but it has not shipped in an official release. The latest official package checked during review was 0.34.0, and it did not contain codegen. The on-disk format can therefore change without a migration contract for released users.

The post-implementation review confirmed the original 14 findings and found more gaps in action fidelity, page identity, generated source, recovery, and warning behavior. This plan replaces the old loose follow-up list. The design was confirmed with the user on 2026-08-12.

## Approach

### Safety contract

Codegen must not guess. If an action has no safe target or a selected format cannot express it, keep the capture fact in the internal model, omit the unsafe output step, and return a structured warning. Never emit an empty selector, a transient `@eN` ref, or a Playwright `body` fallback.

A successful browser action must stay successful if capture fails. Codegen then enters a degraded state, keeps all possible in-memory data, and reports that crash recovery is incomplete. `codegen status` and `codegen stop` must expose capture and persistence failures.

### Versioned append-only journal

Replace the current sidecar plus metadata companion with one `<session>.codegen.jsonl` journal. The journal is the recovery source and is never rewritten or compacted while active.

Each line is a versioned envelope with a contiguous monotonic `sequence`, a record `type`, and typed record data. Successful captured commands also have a monotonic `actionId`; internal steps have monotonic `stepId` values. One successful browser command is one action record, even when it produces several internal steps.

Journal records include:

- `start`, with schema version, title, creation time, initial page state, URL, and viewport
- `action`, with the complete successful action and all pre-action capture data
- Typed, idempotent `update` records for selector enrichment, frame scope, popup binding, page state, and asserted URL
- Durable capture facts and warning resolution records
- `output-written`, after an artifact or response output is ready
- `discard-requested`, before explicit cleanup

The first `start` record must be written before recording becomes active. Append operations open, append, flush, and close the journal. Per-action `fsync` is not required, so the contract covers daemon and process crashes but does not promise recovery after host power loss.

Recovery ignores only one unparsable final line that has no terminating newline. It rejects a malformed complete line, unknown version, missing or duplicate sequence, update for an unknown step, and a journal larger than 64 MiB. An unsupported journal from an unreleased development build becomes `recovery-error`; it is not migrated.

The journal state machine is explicit:

- `inactive`
- `active`
- `restored`
- `degraded`
- `recovery-error`
- `cleanup-pending`

Keep a derived `active` Boolean in structured status output for simple clients. A journal ending with `output-written` or `discard-requested` restores as `cleanup-pending`, not as an active recording.

### Start, stop, recovery, and discard

Any current journal, old journal, or old metadata companion blocks `codegen start`. An active or restored flow must be stopped or discarded. A recovery error or cleanup-pending state must be discarded.

Add `codegen discard` and matching MCP support. It accepts no arguments. It appends and flushes `discard-requested`, then removes only the exact known journal and legacy metadata paths for the current session. It rejects symlinks, directories, and special files. A cleanup failure keeps `cleanup-pending`, reports exact remaining paths, and permits a later discard attempt.

`codegen stop` uses this order:

1. Drain available page events and finalize pending navigation and popup observations.
2. Read and validate the journal, or use the known degraded in-memory model when persistence failed.
3. Render the selected format and its warnings.
4. Write the requested artifact atomically, when a path is present.
5. Append and flush `output-written` with the selected format and optional artifact path.
6. Mark codegen inactive.
7. Remove the journal and known legacy companion, then verify that both are absent.

If rendering, artifact writing, or the terminal journal append fails, keep the recording active so stop can be retried. If cleanup fails after `output-written`, return an unsuccessful command with structured artifact and sidecar paths, and restore later as `cleanup-pending`. When output is returned only in the command response, security cleanup has priority over retrying a response lost during a process crash.

### Safe files

Create a new Unix journal as a regular file with mode `0600` in the open operation. Reject an existing path, symlink, directory, or special file instead of truncating it.

Write artifacts through a temporary file in the destination directory, flush it, and rename it atomically. A new artifact uses mode `0600`. If an artifact already exists, apply its existing permissions to the temporary file before replacement.

The journal is not encrypted. Document the residual risk for another process running as the same local account.

### Exact element capture

Refactor `cli/src/native/element.rs::resolve_element_object_id` into the canonical target seam. Introduce a `ResolvedElement` type that retains the DOM object ID, optional backend node ID, effective CDP session ID, and logical frame ID.

Targeted interactions must resolve once. Click, double-click, tap, hover, fill, set value, type, select, check, uncheck, upload, and element scroll use the same resolved element for both the browser action and codegen capture. Coordinate actions calculate their effective point from that object instead of resolving a second time.

When codegen is active, probe the resolved object before the action can navigate or replace it. Pass private capture data through interaction and handler result types. Do not add capture fields to public command JSON and do not use a shared `last_probe` field. Discard private capture data when the browser action fails.

Compute the frame path before the action. Use the logical frame ID and the effective session, including OOPIF and same-process iframe cases. Do not probe a ref with the active top-level session when its effective session differs.

The target probe produces typed selector candidates and quality data. A test ID is preferred only when it uniquely identifies the resolved element. Playwright can use a unique test ID, role plus accessible name with optional `nth`, unique CSS, or XPath. Recorder receives only valid Recorder selector syntax. The internal model does not store one output format's selector string as canonical data.

Direct `text=` and bare XPath support are not added by this repair because the native resolver does not currently support them as claimed. The supported XPath form remains `xpath=//...`. Semantic locator commands such as `getbytext` remain separate future work and must never record their temporary DOM marker as a durable selector.

### Typed action model

Replace flattened step values with typed actions that preserve successful command intent. Every page-specific action includes a logical page scope. This includes navigation, viewport, press, assertions, and page close.

The internal model distinguishes:

- Navigate, back, forward, and reload
- Fill and set value
- Type, with text, clear mode, and optional delay
- Select, with `Vec<String>` values
- Upload, with the successful action's paths
- Press, with normalized ordered modifiers, final key, scope, and optional asserted URL
- Click-like input, with button, pointer or touch kind, click count, and effective position
- Page wheel scroll and element scroll, using effective calculated coordinates
- Explicit link opening in a new page
- Natural popup creation
- Tab creation, switching, and scoped page close
- Existing element assertions and selector visibility waits
- Viewport and initial browser context properties

`click --new-tab` is explicit page creation, not a natural popup click. Top-level browser `close` is a recording lifecycle event and is not emitted as page intent. `tab close` is the action that becomes a scoped Recorder or Playwright close.

One successful command is one durable action record. This prevents a crash from keeping only a viewport prefix or only `keyDown` from a press.

### Supported and omitted actions

Maintain an exhaustive action classification instead of a silent wildcard:

- Recorded actions: navigation, targeted input, scrolling, viewport, existing element assertions, selector visibility waits, tab lifecycle, upload for Playwright, and browser-close normalization
- Read-only observations: snapshot, screenshot, text and attribute reads, URL and title reads, console inspection, and equivalent non-mutating queries; these need no omission warning
- Omitted mutating or wait actions: record a durable omission fact and expose a format warning

Time, text, URL, load-state, and function wait variants remain future work. Keep current selector visibility waits and existing element assertions. Do not silently convert an unsupported wait to another assertion.

### Logical page identity

Use persisted codegen page IDs such as `p1`, `p2`, and `p3`. They are monotonic within a flow and are never reused. Map a live page to its CDP target ID. Runtime tab IDs such as `t1` remain user-facing data and are not durable identity.

Capture active page scope before command dispatch. The page active at start is `p1` and Recorder target `main`. If codegen starts before a browser exists, the first created or discovered page becomes `p1`. Existing background pages receive logical IDs and initial URLs, but generated output creates them only when a recorded action uses them.

Keep the logical page ID and current target URL separately. Playwright maps by logical page ID. Recorder uses only the target URL for its `target` field. Two pages with the same URL remain distinct in Playwright. Recorder receives a warning when its URL target cannot distinguish them.

Explicit tab commands bind pages from handler results. `click --new-tab` must retain the `tab_new` result it currently discards. Natural popups bind through CDP `openerId` and the pre-action opener target. Ambiguous popup attribution creates a new logical page, omits the causal link, and produces a warning. Generated popup variables are unique per logical page.

On local browser shutdown, keep the last active logical page as unbound. Rebind the first active target after relaunch to that logical page; keep other old pages unbound and give other discovered targets new logical IDs. On external CDP reconnect, reuse a logical page only when the same target ID still exists.

### Navigation assertions

Keep one pending navigation-capable action per logical page. Click, change, and press can receive an automatic URL assertion. Explicit navigate remains a navigate action.

Use successful handler URL data when it exists. Otherwise, read the complete post-action URL from the pre-action page session and process CDP navigation events, including `Page.frameNavigated`, target lifecycle events, and `Page.navigatedWithinDocument`. Redirects update the same pending action to the latest final URL.

Before a new navigation-capable action replaces the pending action on a page, drain and apply available events. At stop, drain events and read current URLs for live pending pages without a fixed sleep. Do not attach an event by guessing. A failed URL check produces `navigation-check-failed` unless a known page close explains the missing target.

Keep one popup-capable click pending per opener page. If a popup arrives after that pending click was replaced, create its page without a causal link and report `ambiguous-popup-origin`.

The repaired target path adds at most three CDP calls for a targeted action in a frame: one `Runtime.callFunctionOn` probe, one `Page.getFrameTree` call only for framed actions, and one `Runtime.evaluate` URL check. Press needs only the URL check. `tab_list()` and private page-list reads are in-memory and are not CDP calls.

### Format policy

Recorder JSON and Playwright can support different action sets. The internal model stays richer than both formats.

Recorder JSON:

- Validate every artifact with pinned `@puppeteer/replay` parsing.
- Emit only non-empty valid Recorder selectors.
- Emit multi-value select, upload, type, explicit page creation, and other unsupported intent as omissions with format warnings.
- Convert back, forward, and reload to observed final navigation only when the URL is available, with a lossy-conversion warning.
- Use current target URL for non-main page scope and warn when same-URL pages are ambiguous.
- Emit scoped `tab close` as Recorder `close`.

Playwright:

- Emit full multi-value `selectOption`, `setInputFiles`, `pressSequentially`, normalized key chords, explicit page creation, natural popups, scoped navigation, scoped viewport, and scoped close.
- Use unique variables for every logical page and popup.
- Render CSS and XPath from canonical selector kinds.
- Render frame indexes through `frameLocator('iframe, frame').nth(index)` only after real-browser tests prove the mapping. Omit and warn if mapping cannot be proved.
- Emit initial context options for viewport, device scale factor, mobile mode, and touch mode. Later width and height changes can use `setViewportSize`. Omit later mobile or scale-mode changes with a warning because an existing context cannot change them.
- Serialize every JavaScript string through a real JSON or JavaScript string serializer. Do not use manual apostrophe replacement.

Generated artifacts do not contain warning comments. Warnings stay in structured command responses so artifacts remain valid for their target tools.

### Warnings and secrets

Return a `warnings` array with stable codes. Warning summaries include a count and up to ten affected action IDs. Add an emitted step number only when the selected format emitted that action. Do not use one ambiguous global step number.

Journal underlying capture facts. Compute format-specific omission and loss warnings during rendering. Transient capture warnings can be resolved by a later journal update; security and format-limit warnings remain.

Warning messages never contain filled text, typed text, selected values, upload contents, or captured credentials. They can include action type, selector class, action ID, page ID, title, and output path.

Always warn that values are stored verbatim when the flow contains fill, type, set value, select, or upload. Add a stronger warning for a confirmed password target. This protects CSS selector flows and probe-failure cases.

`codegen status` reports state, title, journal path, captured action count, internal step count, capture and security warning counts, projected JSON and Playwright omitted and lossy counts, and recovery or cleanup errors. `codegen stop` also reports selected-format emitted, omitted, and lossy counts. A selected format can produce a valid empty artifact with warnings.

### Public command, MCP, and documentation parity

Add `codegen discard` to `cli/src/commands.rs`, `cli/src/native/actions.rs`, `cli/src/mcp.rs`, `cli/src/output.rs`, native parity lists, and the WebDriver support contract. It has no feature arguments. Add CLI and MCP parser-shape parity tests.

Update all user-facing surfaces required by `AGENTS.md` when their behavior changes:

- `cli/src/output.rs`
- `README.md`
- `skill-data/core/SKILL.md`
- `skill-data/core/references/codegen.md`
- `skill-data/core/references/commands.md`
- `docs/src/app/codegen/page.mdx`
- `docs/src/app/commands/page.mdx`
- `docs/src/app/security/page.mdx`
- MCP descriptions and schemas
- Parser usage and error text
- Inline comments in affected source files

Do not add feature content to `skills/agent-browser/SKILL.md`. Do not update either changelog until a later manual release-preparation task.

### Conformance and verification

Pin exact root development dependencies `@puppeteer/replay` 4.0.2 and `@playwright/test` 1.62.1 through pnpm. Add a root `test:codegen-formats` script and a separate Node CI job. Do not make Rust unit tests depend on installed Node packages.

Keep shared golden artifacts under `cli/src/native/codegen/test-fixtures/`. Rust tests compare production renderer output with these fixtures. A root Node test parses the same Recorder JSON with `@puppeteer/replay` and runs Playwright `--list` on the same generated spec.

Shared fixtures cover every supported step variant, hostile string values and line terminators, two natural popups, two same-URL tabs, nested frames, mobile context options, multi-select, upload, modifier chords, scoped navigation, scoped viewport, scoped close, and format omissions.

Behavioral browser tests replace the source-text probe test and the unused `RefMap::add_selector` test. Cover test ID uniqueness, unique ID, positional CSS, unnamed refs, failed probes, CSS password fields, same-process frames, OOPIFs, `<iframe>`, `<frame>`, and nested frame paths.

Journal tests cover partial final records, malformed complete records, sequence errors, unknown-step updates, append failure, degraded recovery, output failure before cleanup, cleanup failure after output, oversized journals, old development files, and symlink or special-file paths. Do not add tombstone tests whose only purpose is to prove that removed code stays absent.

Add command-count tests for the codegen CDP path. Measure codegen-active timing and record it as evidence without a fixed timing threshold.

## Out of scope

- Page-side recording of direct human interactions
- Codegen replay
- New native support for direct `text=` selectors or bare XPath
- Recording semantic locator commands that use temporary DOM markers
- Exact support for time, text, URL, load-state, or function wait variants
- Encryption of local journals
- Dashboard codegen UI
- Release preparation and changelog entries

## Prerequisites

- Node 24 or later, already required by the repository
- pnpm 11.1.3, as declared by the repository
- Chrome for ignored native e2e tests
- No new Rust crate dependency is expected

## Implementation phases

Each phase must keep CLI and MCP behavior aligned and update relevant user documentation in the same change when it changes a public contract.

### Phase 1: Durable journal, recovery states, cleanup, and discard

- Replace `cli/src/native/codegen/sidecar.rs` with the versioned append-only journal and strict recovery validation.
- Replace `CodegenState.active` with the explicit state machine while keeping a derived structured `active` field.
- Implement safe create, append, terminal records, 64 MiB recovery limit, old-development-file detection, and exact-path cleanup.
- Implement atomic protected artifact writing and stop retry semantics.
- Add `codegen discard` across CLI, daemon, MCP, output, native parity, WebDriver support, and required documentation surfaces.
- Add journal, recovery, cleanup, parser, MCP parity, output, and file-safety tests.
  **Commit:** `fix(codegen): make recording recovery and cleanup durable`

### Phase 2: Exact target capture and typed action model

- Introduce `ResolvedElement` in `cli/src/native/element.rs` and make targeted interactions use one exact resolution.
- Probe before successful targeted actions only when codegen is active, using the effective session and pre-action frame scope.
- Pass private capture data through interaction and handler results without changing public command JSON.
- Replace flattened steps with typed action groups, typed selectors, normalized key chords, pointer data, exact select values, upload paths, effective scroll data, and scoped viewport and navigation.
- Remove unsafe empty selector and Playwright `body` fallbacks. Omit unsafe format steps with structured warnings.
- Add the exhaustive recorded, observation, and omitted-action support matrix.
- Add behavioral selector, password, frame, pointer, type, select, upload, scroll, and command-count tests.
  **Commit:** `refactor(codegen): capture exact action intent and page scope`

### Phase 3: Logical pages, popups, navigation, and relaunch

- Add persisted codegen page IDs and runtime target bindings.
- Capture page identity before dispatch and use handler page data for tab lifecycle commands.
- Separate explicit new-page opening from natural popup clicks.
- Retain CDP popup opener data and bind popups without URL or recency guesses.
- Add pending navigation and popup actions, same-document navigation handling, redirect updates, final stop checks, and failure warnings.
- Implement local relaunch and external reconnect identity rules.
- Normalize top-level browser close and emit scoped close only for `tab close`.
- Add same-URL tab, multiple popup, ambiguous popup, scoped navigation, tab close, browser relaunch, external reconnect, and late-navigation tests.
  **Commit:** `fix(codegen): preserve page and navigation identity`

### Phase 4: Faithful Recorder and Playwright emitters

- Render each format from the typed model with the agreed exact, lossy, and omitted-action rules.
- Use real JavaScript string serialization, unique page variables, exact selector syntax, full Playwright select and upload values, type behavior, chords, pointer data, page scope, frame scope, and context options.
- Add pinned `@puppeteer/replay` and `@playwright/test` development dependencies, shared golden fixtures, the root conformance test, the root script, and the separate CI job.
- Validate Recorder schema, Playwright source collection, hostile strings, all supported action variants, omissions, same-URL tabs, multiple popups, frames, and mobile context behavior.
- Update user-facing format support documentation in the same change.
  **Commit:** `fix(codegen): generate schema-valid faithful test artifacts`

### Phase 5: Documentation and end-to-end verification

- Complete the CLI, MCP, README, core skill, detailed reference, docs site, security, and inline-comment audit.
- Add or update native ignored Chrome e2e tests for the complete happy path and the high-risk recovery, selector, password, navigation, popup, tab, frame, and cleanup paths.
- Run all required Rust, Node, docs, parity, and ignored e2e checks.
- Record the codegen-active timing measurement and the actual CDP command counts.
- Confirm that no dashboard or changelog changes were added.
  **Commit:** `docs(codegen): document durable flow generation`

## Required verification

- `cargo test --manifest-path cli/Cargo.toml`
- `cargo fmt --manifest-path cli/Cargo.toml -- --check`
- `cargo clippy --manifest-path cli/Cargo.toml -- -D warnings`
- `pnpm test:codegen-formats`
- `pnpm --dir docs lint`
- `pnpm --dir docs build`
- `cargo test --manifest-path cli/Cargo.toml e2e -- --ignored --test-threads=1`

Dashboard checks are not required because the dashboard has no codegen surface.

## Risks and tradeoffs

- Recorded journals and artifacts contain typed values verbatim. Mode `0600`, protected paths, atomic cleanup, universal typed-value warnings, and stronger password warnings reduce but do not remove local-account risk.
- Append and flush protects against process failure but not every host power failure. Per-action `fsync` is rejected because of its latency cost.
- Recorder JSON cannot represent all Playwright-supported actions. Explicit omission and loss warnings are safer than valid-looking incorrect steps.
- Recorder identifies non-main targets by URL. It cannot distinguish two pages on the same URL, so Playwright remains exact while Recorder reports the ambiguity.
- Logical page recovery across a local browser relaunch preserves recording continuity but cannot recreate closed background pages. Those pages stay unbound until an exact identity is available.
- Targeted actions in frames can add up to three CDP calls during active recording. Inactive codegen adds no capture calls.
- Frame index parity between CDP and Playwright must be proved with real browser fixtures. Unproved cases are omitted with warnings.
- Keeping degraded capture in memory permits useful stop output but cannot make unjournaled actions recoverable after daemon loss.

## Open questions

None. The user confirmed the shared design on 2026-08-12. Implementation is not authorized until the revised planning files receive separate approval.
