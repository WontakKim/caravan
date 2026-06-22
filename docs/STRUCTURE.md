# Caravan — Internal Structure Guide

This document describes the on-disk layout of the Caravan repository after the
initial POC refactoring pass (T-1 through T-4). Its purpose is to make the
chosen structure and the splitting criteria explicit for the next contributor.

---

## 1. Per-Crate Responsibilities

The workspace contains three crates under `crates/`:

| Crate    | Responsibility |
|----------|----------------|
| `kernel` | Core execution and model logic: event log, conversation transcript, prompt compilation, model routing, tool harness, and storage. Has no dependency on any terminal or UI library. |
| `tui`    | Terminal user-interface built on [ratatui](https://github.com/ratatui-org/ratatui). Owns the `App` state machine, the key-event input handler, and the drawing layer. Depends on `kernel` for types and execution. |
| `cli`    | Binary entry point. Wires terminal setup (raw mode, alternate screen, panic hook) and runs the ratatui event loop. Has no business logic of its own; delegates everything to `tui` and `kernel`. |

---

## 2. Kernel Internal Module Families

```
crates/kernel/src/
├── approval.rs          # Approval gate types: ApprovalRequirement / ApprovalGate / ApprovalRequest / ApprovalDecision / ApprovalDecisionRecord / ParsedApprovalRequest / ApprovalResumePlan / ApprovalResumeRecord (pure data; ApprovalDecision is the governance trace that resolves a referenced ApprovalRequest; ApprovalDecisionRecord formats and parses the decision detail string; ParsedApprovalRequest parses an ApprovalRequest event detail string into tool/path/risk/reason fields and converts to ToolRequest; ApprovalResumePlan ties a ParsedApprovalRequest to its request_seq and decision_seq for resume execution; ApprovalResumeRecord formats and parses the ApprovalResume event detail string — resume_detail carries request_seq, decision_seq, tool name, and path; evaluated by ToolEventRunner after ToolPolicy — /approval approve <seq> / /approval reject <seq> append an ApprovalDecision without resuming tool execution; /approval resume <seq> records ApprovalResume then executes the tool)
├── approval_queue.rs    # ApprovalQueue projection over the EventLog: pending (ApprovalRequest events with no valid matching ApprovalDecision) and resolved (ApprovalRequest events resolved by a valid ApprovalDecision event) partitions; a decision is valid when it parses via ApprovalDecisionRecord, references an existing ApprovalRequest seq, and its own seq > request seq (greatest decision seq wins on ties); /approval status shows the pending list and the resume_plans() projection; ApprovalQueue::resume_plans() is a read-only projection of resolved approvals whose decision is Approved and whose tool is supported (read_file / list_files) — each surviving entry is an ApprovalResumePlan carrying request_seq, decision_seq, request_detail, and the parsed ParsedApprovalRequest; consumed plans (those whose request_seq has been referenced by a recorded ApprovalResume event) are excluded from resume_plans() — the plan is consumed on first attempt even if the tool fails and will not reappear in /approval status; no tool execution is performed in this step
├── commands.rs          # Facade: re-exports from commands/ submodule
├── commands/            # commands submodule
│   ├── types.rs         # Command enum + ParsedInput
│   ├── parse.rs         # Command parsing logic
│   └── tests.rs         # Unit tests
├── events.rs            # Facade: re-exports from events/ submodule
├── events/              # events submodule
│   ├── ids.rs           # EventSeq, RunId, TurnId
│   ├── kind.rs          # EventKind + name()
│   ├── record.rs        # AppEvent
│   ├── log.rs           # EventLog
│   └── tests.rs         # Unit tests
├── manual_context.rs    # ManualToolContext: user-attached tool context blobs
├── model_config.rs      # Facade: pub use crate::model::config::*
├── model_gateway.rs     # Facade: pub use crate::model::gateway::*
├── model_registry.rs    # Facade: pub use crate::model::registry::*
├── model_runtime_config.rs  # Facade: pub use crate::model::runtime_config::*
├── model_tool_request.rs    # Facade: pub use crate::model::tool_request::*
├── model_types.rs       # Facade: pub use crate::model::types::*
├── prompt.rs            # Prompt compilation: transcript + manual context + static ToolCatalog section → prompt string
├── runner.rs            # Turn execution orchestrator (run_mock_turn, MockRunOutput)
├── storage.rs           # EventStore: JSONL persistence for EventLog
├── transcript.rs        # ConversationTranscript / TranscriptMessage / TranscriptRole
│
├── model/               # Model family (canonical home; top-level model_* are facades into here)
│   ├── mod.rs           # ModelAdapter trait + ModelRequest/Output/Error/Usage (core); submodule decls + root re-exports
│   ├── config.rs        # ModelConfig / ModelProfile (static configuration)
│   ├── gateway.rs       # ModelGateway / ModelResponse / ModelRoute (routing logic)
│   ├── gateway/         # gateway submodule
│   │   └── tests.rs     # Unit tests
│   ├── registry.rs      # Adapter registry: maps ModelAdapterKind → ModelAdapter impl
│   ├── runtime_config.rs    # ModelRuntimeConfig loaded from process environment
│   ├── runtime_config/      # runtime_config submodule
│   │   └── tests.rs         # Unit tests
│   ├── tool_request.rs  # ModelToolRequest parsed from model output
│   ├── types.rs         # ModelAdapterKind / ModelProvider enums
│   └── openai/          # OpenAI-compatible HTTP adapter
│       ├── mod.rs
│       ├── compatible.rs  # OpenAICompatibleAdapter (ModelAdapter impl)
│       ├── config.rs      # Provider-specific URL / auth config
│       ├── http.rs        # StubOpenAIHttpClient + BlockingOpenAIHttpClient (synchronous; no async/tokio)
│       ├── http/          # http submodule
│       │   └── tests.rs   # Unit tests (T-3: test extraction only; production code unchanged)
│       ├── request.rs     # Request serialisation helpers
│       └── types.rs       # OpenAI wire types (ChatCompletionRequest, etc.)
│
└── tool/                # Tool harness family
    ├── mod.rs           # Module declarations
    ├── events.rs        # ToolEventRunner: executes a tool and records events
    ├── events/          # events submodule
    │   ├── detail.rs    # Event detail string formatters for ToolCall/ToolResult/ToolError
    │   └── tests.rs     # Unit tests for ToolEventRunner and detail formatters
    ├── policy.rs        # ToolPolicyEngine / ToolPolicyDecision / ToolPolicyOutcome
    ├── registry.rs      # ToolRegistry, ToolRequest, ToolOutput, ToolName, ToolRisk
    ├── registry/        # registry submodule
    │   ├── path.rs      # Workspace path confinement helper (resolve_in_workspace) for safe in-workspace resolution
    │   └── tests.rs     # Unit tests for ToolRegistry and path-safety logic
    └── schema.rs        # ToolSpec, ToolInputSpec, ToolCatalog
```

The `commands/` sub-directory follows the same facade pattern as `events/`:
`commands.rs` re-exports from `commands/types.rs` (Command enum + ParsedInput),
`commands/parse.rs` (parsing logic), and `commands/tests.rs` (unit tests).

The `tool/` and `model/openai/` sub-directories were introduced in this POC to
give each family a private namespace and prevent flat-file sprawl at the
`kernel/src/` level.

The six top-level `model_*.rs` files are now thin compatibility facades
(`pub use crate::model::<sub>::*;`) over canonical submodules consolidated under
`model/`. The canonical paths are `kernel::model::{config, gateway, registry,
runtime_config, tool_request, types}`; the legacy `kernel::model_*` module paths
(e.g. `kernel::model_gateway::ModelGateway`) and the kernel root re-exports
(`kernel::ModelGateway`, `kernel::ModelRuntimeConfig`, …) continue to resolve
unchanged through the facades. Core adapter types (`ModelAdapter`,
`ModelRequest`/`Output`/`Error`/`Usage`) still live in `model/mod.rs`.

---

## 3. TUI Internal Module Families

```
crates/tui/src/
├── app.rs          # App struct, constructors, high-level submit() dispatcher, and help_lines
├── app/
│   ├── approval.rs  # handle_approval_command: /approval status, /approval approve <seq>, /approval reject <seq>, /approval resume <seq> (resume branch records ApprovalResume, consumes the plan, then runs ToolPolicy → ToolCall → ToolResult|ToolError)
│   ├── context.rs   # handle_context_command: /context attach-last-tool, clear, status
│   ├── logging.rs   # screen-log formatting helpers
│   ├── request.rs   # handle_request_command: /request status, run, clear
│   ├── selection.rs # navigation: select_next/select_prev, scroll_inspector_down/up
│   ├── tests.rs     # Aggregator: 11 `mod` declarations only; no test bodies (cfg(test))
│   ├── tests/
│   │   ├── common.rs      # Shared test helpers (TempDir RAII, TEST_COUNTER)
│   │   ├── lifecycle.rs   # App construction and teardown tests
│   │   ├── commands.rs    # Slash-command dispatch tests
│   │   ├── storage.rs     # EventStore / persistence tests
│   │   ├── selection.rs   # Navigation (select_next/prev, scroll) tests
│   │   ├── model_flow.rs  # Model execution flow tests
│   │   ├── tools.rs       # /tool command handler tests
│   │   ├── context.rs     # /context command handler tests
│   │   ├── request.rs     # /request command handler tests
│   │   ├── policy.rs      # Tool-policy decision tests
│   │   └── approval.rs    # /approval status, approve command tests, and reject command tests
│   └── tools.rs     # handle_tool_command: /tool list, /tool read
├── input.rs        # Key-event handler: maps crossterm KeyEvents → App mutations
├── ui.rs           # Layout-orchestration root: draw() calls each widget's render helper; Nav and Main panel blocks remain inline in draw()
└── ui/
    ├── header.rs        # Header text + render
    ├── inspector.rs     # Inspector text/labels + render
    ├── event_log.rs     # Event-log tailing/highlight + render
    └── prompt_bar.rs    # Input display width + prompt bar + cursor
```

`app.rs` is the command-dispatch root: it owns the `App` struct, constructors, a
high-level `submit()` dispatcher that routes each slash command to the appropriate
handler, and the `help_lines` helper. `app/*.rs` is the per-command handler family:
`handle_approval_command` in `app/approval.rs`, `handle_tool_command` in
`app/tools.rs`, `handle_context_command` in `app/context.rs`,
`handle_request_command` in `app/request.rs`, navigation in `app/selection.rs`,
and screen-log formatting helpers in `app/logging.rs`.

`app/tests.rs` is a thin aggregator that contains only the 11 `mod` declarations
for its child modules under `app/tests/`. Each child module owns a focused group
of tests: `common` provides shared helpers; `lifecycle`, `commands`, `storage`,
`selection`, `model_flow`, `tools`, `context`, `request`, `policy`, and
`app/tests/approval.rs` each own the tests for the corresponding `App` behaviour.
The aggregator is co-located with the module it tests rather than placed in
`crates/tui/tests/`, which would require making internals `pub`. The public
integration-test suite lives in `crates/tui/tests/public_api.rs`.

`ui.rs` is the layout-orchestration root: its `draw()` function computes the
overall screen layout and calls each widget's `render` helper. `ui/*.rs` is the
per-widget render family: `ui/header.rs` owns header text and render,
`ui/inspector.rs` owns inspector text/labels and render, `ui/event_log.rs` owns
event-log tailing, highlight logic, and render, and `ui/prompt_bar.rs` owns input
display width, the prompt bar, and cursor render. Each widget's text/compute
helper and its tests are co-located in the child module.

---

## 4. CLI Responsibility

`crates/cli/src/main.rs` is intentionally thin:

1. Install a panic hook that restores the terminal before printing the panic.
2. Load `ModelRuntimeConfig` from environment variables; exit early on error.
3. Create the `ModelGateway` and the store-backed `App`.
4. Enable raw mode and enter the alternate screen.
5. Run the ratatui event loop (`run_app`).
6. Unconditionally restore the terminal on exit or error.

No business logic lives here. All application behaviour belongs in `tui` or
`kernel`.

---

## 5. EventLog Principles

`EventLog` is an **execution trace**, not a UI view model. The following
constraints are enforced:

- **Execution events only.** `EventLog` records what the system *did*:
  `AppStart`, `UserMessage`, `RunCreate`, `TurnStart`, `PromptCompile`,
  `ModelRoute`, `ModelOutputChunk`, `AssistantMessage`, `ModelUsage`,
  `RunComplete`, `RunFail`, `ModelError`, `ModelToolRequest`, `ToolPolicy`,
  `ApprovalRequest`, `ApprovalDecision`, `ApprovalResume`, `ToolCall`, `ToolResult`, `ToolError`,
  `ToolContextAttach`, `ToolContextClear`, `SlashCommand`, `HelpRequest`,
  `LogClear`, `ExitRequest`, `UnknownSlashCommand`.
- **No UI selection, scroll, or focus events.** Events such as "user moved the
  inspector cursor" or "user scrolled the event pane" are pure view state and
  must not enter the log.
- **No `InspectorSelection` events.** TUI-local navigation is not part of the
  execution trace.
- **`ToolResult` stores no full file content.** The `detail` field of a
  `ToolResult` event is a summary (e.g. `tool=read_file status=ok`), not the
  raw file bytes. Large payloads are passed through the in-memory
  `ManualToolContext`; they are never persisted inside `EventLog`.
- **API keys are never recorded.** No event detail may contain authentication
  credentials. `ModelRuntimeConfig` holds keys only in memory and they are
  never stringified into an `AppEvent`.

---

## 6. Model / Tool / Prompt / TUI Boundaries

```
┌─────────────────────────────────────────────────────┐
│  cli  (binary: terminal setup + event loop only)    │
│          │                                           │
│         tui  (App state + input + drawing)           │
│          │                                           │
│        kernel                                        │
│    ┌─────┴──────────────────────────┐               │
│    │  prompt   transcript   runner  │               │
│    │     │                   │      │               │
│    │   model/           tool/       │               │
│    │  openai/    EventLog  storage  │               │
│    └────────────────────────────────┘               │
└─────────────────────────────────────────────────────┘
```

- **Prompt** builds the prompt string from `ConversationTranscript`,
  `ManualToolContext`, and the static read-only `ToolCatalog` prompt section
  (`tool::schema`); it produces the prompt text placed in the `ModelRequest`. It
  does not touch the event log.
- **Model (`model/openai/`)** knows only the wire protocol. It receives a
  `ModelRequest` and returns `ModelOutput`. It has no knowledge of tools,
  prompts, or the event log.
- **Tool (`tool/`)** owns the harness: schema, registry, policy, and event
  runner. It records `ToolPolicy` / `ApprovalRequest` / `ToolCall` /
  `ToolResult` / `ToolError` events but does not interact with the model layer
  directly. The **Approval Gate** (`crates/kernel/src/approval.rs`) sits
  between `ToolPolicy` and `ToolCall`: after `ToolPolicyEngine` produces a
  `ToolPolicyOutcome` (including the `approval_requirement` field), the
  `ApprovalGate` evaluates that requirement and returns
  `Some(ApprovalRequest)` when manual approval is needed (emitting an
  `ApprovalRequest` event) or `None` when the tool may proceed immediately. On
  the production path (`ToolPolicyEngine::read_only()`), `approval_requirement`
  is always `ApprovalRequirement::None`, so no `ApprovalRequest` event is
  emitted and tools run without an approval step.
- **`/tool plan-write <path>`** exercises the mutation-intent → `ToolPolicy` →
  `ApprovalRequest` path directly: it records a `workspace_write` mutation intent
  and routes it through the approval gate, but performs no real file write and
  produces no `ToolCall`/`ToolResult`/`ToolError`. Because no tool is executed,
  there is no resume candidacy; the request is resolved only via
  `/approval approve|reject <seq>`.
- **TUI** reads from `EventLog` and `App` state to draw the screen. It writes
  to `App` state through `input::handle_key`. It never reaches into
  `model/openai/` internals or tool registry internals.

---

## 7. What Was Split This POC

| Before | After | Reason |
|--------|-------|--------|
| Tool logic scattered across flat `kernel/src/` files | `tool/` sub-directory with `events.rs`, `policy.rs`, `registry.rs`, `schema.rs` | Tool harness is a cohesive family with clear internal boundaries |
| OpenAI adapter spread across flat `kernel/src/` files | `model/openai/` sub-directory with `compatible.rs`, `config.rs`, `http.rs`, `request.rs`, `types.rs` | HTTP client, wire types, and config belong together; isolates provider details from the adapter trait |
| `App` tests inside `app.rs` or absent | `app/tests.rs` aggregator + `app/tests/{common,lifecycle,commands,storage,selection,model_flow,tools,context,request,policy}.rs` child modules | Keeps tests co-located without polluting the main module file; avoids forcing internal APIs public; grouping by concern keeps each child module focused and independently navigable |
| Per-command handlers inline in `app.rs` | `app/{tools,context,request,selection,logging}.rs` handler family | Each handler family has a distinct responsibility (`/tool`, `/context`, `/request`, navigation, screen-log formatting); extracting them gives `app.rs` a clean dispatch-root role |
| `runner.rs` tests inline in the production module | `runner/tests.rs` extracted alongside `runner.rs` | Co-locates tests with the module they exercise without crowding the production code; mirrors the `app/tests.rs` pattern |
| `ui.rs` single drawing file | `ui/{header,inspector,event_log,prompt_bar}.rs` render modules | Each widget's render + its text/compute helper + its tests now has a clear home; `draw()` becomes layout orchestration |
| `tool/events.rs` flat module | `events/` subdir: `detail.rs` (event detail string formatters) + `tests.rs` (unit tests) | Isolates detail formatting logic from the runner; keeps tests co-located without crowding the production module |
| `tool/registry.rs` flat module | `registry/` subdir: `path.rs` (workspace path confinement helper) + `tests.rs` (unit tests) | Extracts workspace-root path confinement into a focused module; rejects absolute paths and `..`, canonicalizes root and candidate paths, and verifies the canonical candidate remains under the canonical workspace root to guard against symlink escape; co-locates tests alongside the code they exercise |
| `events.rs` flat module | `events/` subdir: `ids.rs, kind.rs, record.rs, log.rs, tests.rs` | Separate the id/seq, kind, record, and log responsibilities; co-locate tests |
| `model_runtime_config.rs` inline tests | `model_runtime_config/tests.rs` extracted alongside `model_runtime_config.rs` (T-1: **test extraction only** — production code was not split) | Co-locates tests with the module they exercise without crowding the production file; mirrors the `runner/tests.rs` pattern |
| `model_gateway.rs` inline tests | `model_gateway/tests.rs` extracted alongside `model_gateway.rs` (T-2: **test extraction only** — production code was not split) | Co-locates tests with the module they exercise without crowding the production file; mirrors the `runner/tests.rs` pattern |
| `model/openai/http.rs` inline tests | `model/openai/http/tests.rs` extracted alongside `http.rs` (T-3: **test extraction only** — production code was not split) | Co-locates tests with the module they exercise without crowding the production file; mirrors the `runner/tests.rs` pattern |

---

## 8. What Was Deliberately NOT Split

Some files were left as single flat modules despite their length. This was an
explicit decision, not an oversight:

| File | Reason not split |
|------|-----------------|
| `model/mod.rs` core types → `model/core.rs` | The core adapter contract (`ModelAdapter`, `MockModelAdapter`, `ModelRequest`/`Output`/`Error`/`Usage`, `ModelResult`) remains inline in `model/mod.rs`. Extracting it into `model/core.rs` was deferred because the consolidation of the six `model_*` flat files (see §7) already touches every model-family module and lib.rs; splitting core in the same pass would add import churn without a clear boundary win. |

The earlier `model_*` flat files have since been consolidated into the `model/`
family (see §7 and §9); only the `model/mod.rs` core split above remains
deliberately deferred.

---

## 9. Deferred Refactors

The following refactors were identified but intentionally deferred from this
POC pass. Each entry includes the reason it was left for a later iteration.

| Refactor | Reason deferred |
|----------|-----------------|
| `model_*` family consolidation into `model/` | **DONE** — All six top-level `model_config.rs`, `model_gateway.rs`, `model_registry.rs`, `model_runtime_config.rs`, `model_tool_request.rs`, and `model_types.rs` files were folded into canonical `model::{config, gateway, registry, runtime_config, tool_request, types}` modules (`gateway` and `runtime_config` keep their split `tests.rs` under matching subdirs). The top-level files remain as `pub use crate::model::<sub>::*;` compatibility facades, so `kernel::model_*` module paths and the kernel root re-exports are unchanged. Core adapter types still live in `model/mod.rs` — see the `model/core.rs` deferral in §8. |
| `model/core.rs` extraction | Pull `ModelAdapter`/`MockModelAdapter`/`ModelRequest`/`Output`/`Error`/`Usage`/`Result` out of `model/mod.rs` into `model/core.rs`. Deferred from the consolidation pass to limit import churn; revisit once the core contract grows or a second adapter family lands. |
| `events.rs` split | **DONE** — `events.rs` was split into `ids.rs` (EventSeq/RunId/TurnId), `kind.rs` (EventKind + name()), `record.rs` (AppEvent), `log.rs` (EventLog), and `tests.rs` (unit tests) under `events/`. |
| Nav / Main panel blocks in `draw()` | The Nav and Main panel blocks were intentionally left inline in `draw()` because they are small static/literal blocks with no compute helper; a separate file would add navigation cost without clarity. |
| `app/tests.rs` grouping into child modules | **DONE** — The `App` tests were distributed across 11 child modules under `app/tests/` (`approval`, `common`, `lifecycle`, `commands`, `storage`, `selection`, `model_flow`, `tools`, `context`, `request`, `policy`). `app/tests.rs` is now a thin aggregator containing only the 11 `mod` declarations. No tests remain in the aggregator and no grouping candidates are deferred. |
| `tool/registry/types.rs` split | `ToolRegistry`, `ToolRequest`, `ToolOutput`, `ToolName`, and `ToolRisk` remain in `registry.rs`. Splitting the type definitions into a separate file would require re-exporting them through `registry.rs` or changing all existing import paths across the crate. Defer until the type set grows large enough that the boundary becomes unambiguous. |
| `tool/registry/execute.rs` split | The execution path in `registry.rs` is tightly coupled to its type definitions; separating them now would fragment a small module without a meaningful responsibility boundary and cause public-API import churn. Revisit if dispatch logic grows substantially or diverges in ownership. |
| `commands/parse.rs` per-family split | If `/model`, `/agent`, or approval command families grow substantially, `parse.rs` can be split into `parse_tool.rs`, `parse_context.rs`, `parse_request.rs`, and `parse_model.rs` — one parser per command family. Defer until the command family boundary becomes unambiguous. |
| `model/runtime_config` production split into error/env/parser | `model/runtime_config.rs` mixes error types, environment-variable loading, and config parsing. A future split into `error.rs`, `env.rs`, and `parser.rs` sub-modules would give each responsibility a clean home. Defer until the module grows large enough that the boundaries are unambiguous. |
| `model/gateway` production split once gateway routing grows | `model/gateway.rs` currently holds `ModelGateway`, `ModelResponse`, and `ModelRoute` in a single file. A production split makes sense once gateway routing logic grows (e.g. per-provider dispatch, fallback logic, or load-balancing); defer until the routing grows enough to justify a subdir. |
| `model/openai/http` production split once async/streaming/client variants are added | `http.rs` currently contains `StubOpenAIHttpClient` and `BlockingOpenAIHttpClient` as a synchronous stub and blocking client in one file. When async or streaming variants are introduced, split into dedicated modules (e.g. `async.rs`, `streaming.rs`, `client.rs`). Defer until those variants exist. |
| `write_file` execution and sandbox | Safety design documented in [docs/WRITE_SANDBOX.md](WRITE_SANDBOX.md); `write_file` execution and the filesystem sandbox are not yet implemented. Defer until the mutation path is ready for end-to-end wiring. |

---

## 10. Guiding Principle

> **File splitting is not the goal. It is a means of expressing responsibility
> boundaries.**

A module boundary is justified when it lets you answer the question *"what does
this file own?"* more precisely than the parent file did. Splitting for line
count alone adds navigation cost without adding clarity. Every boundary
introduced in this POC corresponds to a distinct responsibility:

- `tool/` — tool harness (schema, registry, policy, execution)
- `model/openai/` — one HTTP-based provider adapter (wire protocol, auth, config)
- `app/tests.rs` — tests co-located with the module they exercise

Where a clear responsibility boundary did not emerge, the file was left intact.
The deferred items above are candidates for future splits only once their
responsibility boundary becomes unambiguous.
