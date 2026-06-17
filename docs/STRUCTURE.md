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
├── commands.rs          # Parsed user input: Command enum + ParsedInput
├── events.rs            # EventLog, AppEvent, EventKind, EventSeq, RunId, TurnId
├── manual_context.rs    # ManualToolContext: user-attached tool context blobs
├── model_config.rs      # ModelConfig / ModelProfile (static configuration)
├── model_gateway.rs     # ModelGateway / ModelResponse / ModelRoute (routing logic)
├── model_registry.rs    # Adapter registry: maps ModelAdapterKind → ModelAdapter impl
├── model_runtime_config.rs  # ModelRuntimeConfig loaded from process environment
├── model_tool_request.rs    # ModelToolRequest parsed from model output
├── model_types.rs       # ModelAdapterKind / ModelProvider enums
├── prompt.rs            # Prompt compilation: ConversationTranscript → API messages
├── runner.rs            # Turn execution orchestrator (run_mock_turn, MockRunOutput)
├── storage.rs           # EventStore: JSONL persistence for EventLog
├── transcript.rs        # ConversationTranscript / TranscriptMessage / TranscriptRole
│
├── model/               # Model adapter family
│   ├── mod.rs           # ModelAdapter trait, ModelRequest/Output/Error/Usage
│   └── openai/          # OpenAI-compatible HTTP adapter
│       ├── mod.rs
│       ├── compatible.rs  # OpenAI-compatible provider detection
│       ├── config.rs      # Provider-specific URL / auth config
│       ├── http.rs        # Blocking + async HTTP clients, StubOpenAIHttpClient
│       ├── request.rs     # Request serialisation helpers
│       └── types.rs       # OpenAI wire types (ChatCompletionRequest, etc.)
│
└── tool/                # Tool harness family
    ├── mod.rs           # Module declarations
    ├── events.rs        # ToolEventRunner: executes a tool and records events
    ├── policy.rs        # ToolPolicyEngine / ToolPolicyDecision / ToolPolicyOutcome
    ├── registry.rs      # ToolRegistry, ToolRequest, ToolOutput, ToolName, ToolRisk
    └── schema.rs        # ToolSpec, ToolInputSpec, ToolCatalog
```

The `tool/` and `model/openai/` sub-directories were introduced in this POC to
give each family a private namespace and prevent flat-file sprawl at the
`kernel/src/` level.

---

## 3. TUI Internal Module Families

```
crates/tui/src/
├── app.rs          # App struct (state machine) + command-dispatch methods
├── app/
│   └── tests.rs    # Integration-style tests for App behaviour (cfg(test))
├── input.rs        # Key-event handler: maps crossterm KeyEvents → App mutations
└── ui.rs           # Drawing layer: maps App state → ratatui Frame widgets
```

`app/tests.rs` is co-located with the module it tests rather than placed in
`crates/tui/tests/`, which would require making internals `pub`. The public
integration-test suite lives in `crates/tui/tests/public_api.rs`.

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
  `ToolCall`, `ToolResult`, `ToolError`, `ToolContextAttach`,
  `ToolContextClear`, `SlashCommand`, `HelpRequest`, `LogClear`,
  `ExitRequest`, `UnknownSlashCommand`.
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

- **Prompt** knows only `ConversationTranscript` and `ManualToolContext`; it
  produces the message list sent to the model. It does not touch the event log.
- **Model (`model/openai/`)** knows only the wire protocol. It receives a
  `ModelRequest` and returns `ModelOutput`. It has no knowledge of tools,
  prompts, or the event log.
- **Tool (`tool/`)** owns the harness: schema, registry, policy, and event
  runner. It records `ToolCall`/`ToolResult`/`ToolError` events but does not
  interact with the model layer directly.
- **TUI** reads from `EventLog` and `App` state to draw the screen. It writes
  to `App` state through `input::handle_key`. It never reaches into
  `model/openai/` internals or tool registry internals.

---

## 7. What Was Split This POC

| Before | After | Reason |
|--------|-------|--------|
| Tool logic scattered across flat `kernel/src/` files | `tool/` sub-directory with `events.rs`, `policy.rs`, `registry.rs`, `schema.rs` | Tool harness is a cohesive family with clear internal boundaries |
| OpenAI adapter spread across flat `kernel/src/` files | `model/openai/` sub-directory with `compatible.rs`, `config.rs`, `http.rs`, `request.rs`, `types.rs` | HTTP client, wire types, and config belong together; isolates provider details from the adapter trait |
| `App` tests inside `app.rs` or absent | `app/tests.rs` alongside `app.rs` | Keeps tests co-located without polluting the main module file; avoids forcing internal APIs public |

---

## 8. What Was Deliberately NOT Split

Some files were left as single flat modules despite their length. This was an
explicit decision, not an oversight:

| File | Reason not split |
|------|-----------------|
| `events.rs` | `EventLog`, `AppEvent`, `EventKind`, `EventSeq`, `RunId`, and `TurnId` are all part of the same closed type set; splitting gains nothing and would require re-exporting everything. |
| `runner.rs` (test extraction) | The runner tests are small enough to live alongside the production code; extracting them to a separate file adds file-count overhead with no clarity benefit at this size. |
| `ui.rs` | The drawing layer is a single coherent pass over `App` state. Sub-splitting by widget would obscure the single-pass nature and fragment the borrow of `App`. |
| `app.rs` (submit-dispatch handler) | The submit handler is deeply entwined with `App` field mutations; extracting it to a sub-module would only move the coupling, not reduce it. |
| `model_*` flat files → `model/` consolidation | The remaining `model_config.rs`, `model_gateway.rs`, `model_registry.rs`, `model_runtime_config.rs`, `model_tool_request.rs`, and `model_types.rs` files at the `kernel/src/` level were not folded into the `model/` sub-tree; they concern gateway routing and config, not the adapter implementation, so the conceptual boundary does not yet justify a full consolidation. |

---

## 9. Deferred Refactors

The following refactors were identified but intentionally deferred from this
POC pass. Each entry includes the reason it was left for a later iteration.

| Refactor | Reason deferred |
|----------|-----------------|
| Full `model_*` consolidation into `model/` | Gateway, config, and registry modules overlap conceptually with the `model/` sub-tree but each also touches the broader kernel API surface. Consolidating them cleanly requires a broader API audit that is out of scope for this pass. |
| `events.rs` split | All types are a tightly coupled closed set; no clarity gain at current size. Revisit if `EventKind` variants grow beyond ~40 entries or if `EventLog` grows persistence strategies. |
| `runner.rs` test extraction | Test surface is small. Extract to `runner/tests.rs` when the test count makes the file unwieldy (rough threshold: >300 lines of test code). |
| `ui.rs` split | The widget-per-file split is common in ratatui projects but premature here; revisit when the file exceeds ~500 lines or when individual widgets need their own state. |
| `app.rs` submit-dispatch handler extraction | The submit handler is ~80 lines and currently cannot be cleanly extracted without duplicating `App` field borrows. Requires refactoring `App` field access patterns first. |

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
