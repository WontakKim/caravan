# Caravan

Caravan is currently a **Claude Code-like local coding agent baseline** — a Rust TUI shell
that lets you interact with an assistant the same way Claude Code does:

- **Plain text** submits a task to the assistant and runs the mock Run/Turn flow.
- **`CLAUDE.md` project memory** is read from the workspace root at session start and
  injected into the main prompt when present (see `project_memory` module in `crates/kernel`).
- **Slash commands** (`/help`, `/clear`, `/exit`, …) control session state.
- **Tool/approval/write commands** (`/tool`, `/context`, `/request`, `/approval`) are an
  **experimental harness layer** — not the primary user experience. They exist as a
  structural seam for future agentic tooling and are not yet part of the core assistant flow.

> **File mutation is still not implemented.** `/tool plan-write`, `/tool preview-write`, and
> `/tool propose-write` perform no real write to the filesystem. Actual file mutation is
> explicitly deferred to a future task.

> **`CLAUDE.md` may contain secrets.** Caravan loads `CLAUDE.md` from the workspace root and
> injects its content into the model prompt. There is no automatic secret detection — do not
> place API keys, tokens, passwords, or other sensitive values in `CLAUDE.md`.

See [docs/COMMANDS.md](docs/COMMANDS.md) for the full command reference, including which
commands are experimental, reserved, or explicitly unsupported.

---

## Workspace Structure

The repository is a Cargo workspace with a root virtual manifest (`Cargo.toml`)
and three crates under `crates/`:

| Crate | Path | Responsibility |
|-------|------|---------------|
| `kernel` | `crates/kernel` | TUI-free logic: commands, events, storage, prompt compiler, runner, model layer |
| `tui` | `crates/tui` | App state, input handling, and rendering (depends on `kernel`) |
| `cli` | `crates/cli` | Binary entrypoint; produces the `caravan` binary (depends on `kernel` and `tui`) |

See [docs/STRUCTURE.md](docs/STRUCTURE.md) for the internal module layout and refactor criteria.
See [docs/WRITE_SANDBOX.md](docs/WRITE_SANDBOX.md) for the workspace mutation / write-sandbox safety design. The `WriteIntent` data model (`crates/kernel/src/write_intent.rs`) has been added as the first step toward future write tooling, but no actual file write is performed yet. A read-only `WritePreview` dry-run / diff-preview foundation now exists in `crates/kernel`, but actual write execution is not yet implemented.

## Public API

`kernel` re-exports its frequently used domain, model, and runtime types at the crate root; `tui` re-exports `App`.

```rust
use kernel::{EventKind, ModelGateway, ModelRuntimeConfig};
use tui::App;
```

Full module paths (e.g. `kernel::model_gateway::ModelGateway`) remain available for callers that prefer explicit paths. The OpenAI-compatible adapter now calls the HTTP client boundary, but the client is a stub, so no real network calls occur.

## Running

```sh
cargo run
```

```sh
cargo run --bin caravan
```

Run the full workspace test suite:

```sh
cargo test --workspace
```

## Available Commands

| Command              | Description                                              |
|----------------------|----------------------------------------------------------|
| `/help`              | Show the list of available commands                      |
| `/clear`             | Clear the log panel                                      |
| `/exit`              | Exit the application                                     |
| `/tool list [path]`           | List files under the workspace root (or a sub-path)                      |
| `/tool read <path>`           | Read a UTF-8 text file under the workspace root                          |
| `/tool plan-write <path>`     | Approval-only skeleton: records a `workspace_write` mutation intent (`ToolPolicy` + `ApprovalRequest`) without writing any file and produces no `ToolCall`/`ToolResult`/`ToolError`; resolve via `/approval approve\|reject <seq>`; not resumable; see [write-sandbox safety design](docs/WRITE_SANDBOX.md) |
| `/tool preview-write <path>`  | Dry-run diff preview: renders a bounded line-diff preview of what a write to `<path>` would produce, using the latest read-only tool output candidate as proposed content; performs **no write**, creates **no** `ApprovalRequest`, and emits `SlashCommand, ToolPolicy, ToolCall, ToolResult` on success (or `... ToolError` on preview error); the `ToolResult` stores only the content-free `WritePreview::detail()` summary — never any file content or diff lines |
| `/tool propose-write <path>`  | Preview-backed approval request: shows a bounded diff preview and records a `workspace_write` `ApprovalRequest` using the latest tool output as content; performs **no write** |
| `/context attach-last-tool`   | Attach the latest read-only tool output to the next prompt (one-shot)    |
| `/context clear`              | Clear pending manual tool context                                         |
| `/context status`             | Print a read-only status report of pending manual tool context and the last tool-output candidate; does not run the model |
| `/request status`             | Show the pending model tool request: the suggested `/tool` command and the `/context attach-last-tool` next step; does not run the model or any tool |
| `/request run`                | Execute the pending model tool request as a read-only tool; on success records `ToolPolicy` + `ToolCall` + `ToolResult`, shows a preview, updates the manual tool output candidate, clears the pending request, and prompts you to run `/context attach-last-tool`; on failure records `ToolPolicy` + `ToolCall` + `ToolError` and keeps the pending request; with no pending request shows "No pending model tool request." and does not run a tool or the model |
| `/request clear`              | Clear the pending model tool request; does not run the model or any tool |
| `/approval status`            | Show the pending approval queue and approved resume plan summary: lists only **pending** (unresolved) `ApprovalRequest` events, then always prints the count of approved resume plans (`ApprovalQueue::resume_plans` — resolved `Approved` decisions for supported tools), a per-plan summary line (`seq=<n> <detail>`), and a suggested `/tool` command for each plan; observe-only, appends only a `SlashCommand` event; no resume execution happens in this step |
| `/approval approve <seq>`     | Resolve the pending `ApprovalRequest` identified by `<seq>` by appending an `ApprovalDecision` event with detail `request_seq=<seq> decision=approved reason=operator_approved`; if `<seq>` is not pending or has already been resolved, appends nothing and prints `No pending approval for seq=<seq>`; does **not** resume tool execution and emits no `ToolCall`/`ToolResult`/`ToolError` |
| `/approval reject <seq>`      | Resolve the pending `ApprovalRequest` identified by `<seq>` by appending an `ApprovalDecision` event with detail `request_seq=<seq> decision=rejected reason=operator_rejected`; if `<seq>` is not pending or has already been resolved, appends nothing and prints `No pending approval for seq=<seq>`; does **not** resume tool execution and emits no `ToolCall`/`ToolResult`/`ToolError` |
| `/approval resume <seq>`      | Resume an approved `ApprovalResumePlan` identified by `<seq>` as a read-only tool execution; records `ApprovalResume` then `ToolPolicy → ToolCall → (ToolResult \| ToolError)`; does nothing for pending, rejected, or unknown seqs; the resume plan is **consumed on attempt** — even on tool error it is not retried and will no longer appear in `/approval status`; on success run `/context attach-last-tool` to attach the output to the next prompt (`pending_manual_tool_context` is not set automatically) |

> These commands match the in-app `/help` output.

### Header Context Indicator

The TUI header includes a context indicator segment: it shows `| Context: pending`
when manual tool context has been staged and will be attached to the next prompt,
and `| Context: none` when no manual context is staged. Note that merely having a
last tool-output candidate available (i.e. a `/tool read` result that has not yet
been explicitly attached via `/context attach-last-tool`) does **not** set the
indicator to `pending`; only context that has actually been attached and is waiting
to be sent causes the header to display `| Context: pending`.

The header also includes a separate request indicator segment: it shows
`| Request: pending` when a `ModelToolRequest` has been detected and stored as a
pending suggested action that has not yet been cleared, and `| Request: none`
otherwise. The request indicator reflects only the in-memory pending state, which is
set when a `ModelToolRequest` is detected in a model response and is cleared by
`/request clear`; the read-only `/request status` command displays this state without
changing it. The request indicator is independent of the context indicator.

Plain text (any input not starting with `/`) is treated as a user message and runs
the Mock Run/Turn flow, producing `User:` / `Assistant:` output in the Main panel.

## In-Memory Event System

Caravan includes an append-only in-memory event log that records every significant
action as it occurs. Events are also persisted to disk (see [Event Persistence](#event-persistence))
so that the log survives process restarts.

### Event Log Panel

The lower section of the TUI displays the **Event Log** panel. Each row shows:

```
<seq>  <EventKind>
```

- **seq** — a monotonically increasing sequence number starting at `1`. Every
  appended event receives the next integer.
- **EventKind** — the kind of event (see table below).

The currently selected row is highlighted with reversed video.

### Inspector Panel

The right-hand **Inspector** panel shows the details of the selected event:

```
seq: <n>
kind: <EventKind>
message: <detail string>
```

When no event is selected the panel displays `No event selected`.

### Navigation

Use the **Up** and **Down** arrow keys to move the selection through the Event
Log. Navigation is pure UI state — it does not append any event to the log.

- **Down** — move to the next (newer) event; no-op at the bottom boundary.
- **Up** — move to the previous (older) event; no-op at the top boundary.

Use **PageUp** and **PageDown** to scroll the **Inspector** panel vertically
when its content exceeds the visible area.

- **PageUp** — scroll the Inspector panel up.
- **PageDown** — scroll the Inspector panel down.

Inspector scroll is pure UI state — it is not recorded in the event log and
not persisted to `.caravan/events.jsonl`.

### EventKind Values

| EventKind                    | When it is recorded                                      |
|------------------------------|----------------------------------------------------------|
| `AppStart`                   | Once, when the application initialises                   |
| `SlashCommand`               | Recorded for slash commands only (not plain text)        |
| `HelpRequest`                | When `/help` is processed                                |
| `UserMessage`                | When plain (non-command) text is submitted               |
| `LogClear`                   | When `/clear` is processed                               |
| `ExitRequest`                | When `/exit` is processed or Ctrl+C is pressed           |
| `UnknownSlashCommand`        | When an unrecognised `/command` is entered               |
| `RunCreate`                  | When a new Run is initialised for a submitted user message|
| `RunStart`                   | When the Run begins executing (before the first Turn)    |
| `TurnStart`                  | When a Turn begins within a Run                          |
| `PromptCompile`              | When the prompt compiler assembles the structured prompt; `detail` holds the compiled prompt preview |
| `ModelRoute`                 | After `PromptCompile`, before the first `ModelOutputChunk`; carries mock provider/model/adapter route metadata selected by `ModelGateway` |
| `ModelOutputChunk`           | Each incremental model-output chunk (not a tokenizer token; chunks are whitespace-split output fragments or future streaming deltas) |
| `AssistantMessage`           | The final assistant response trace appended on a successful run, after all `ModelOutputChunk` events and before `RunComplete`; not emitted on the error path |
| `RunComplete`                | When the Run finishes successfully                       |
| `RunFail`                    | Emitted when a Run fails on the model error path (after a `ModelError` event); no `ModelOutputChunk` or `RunComplete` is appended |
| `ModelError`                 | Emitted when the model layer returns an error; carries `kind=... message="..."` detail |
| `ToolContextAttach`          | When `/context attach-last-tool` successfully stages a recent tool output as pending context; detail is summary-only (no raw tool output). Not emitted when there is no recent tool output to attach |
| `ToolContextClear`           | When `/context clear` is processed; any pending manual tool context is discarded |
| `ToolPolicy`                 | Policy decision trace recorded immediately before the Approval Gate; carries the tool name, path, risk level, decision, and reason |
| `ApprovalRequest`            | Emitted by the Approval Gate when `approval_requirement` is `Manual`; sits between `ToolPolicy` and `ToolCall`; never emitted on the production read-only-tool path; pending `ApprovalRequest` events can be observed via `/approval status` (the `ApprovalQueue` projection) |
| `ApprovalDecision`           | Governance trace that resolves a referenced `ApprovalRequest`; carries `request_seq`, `decision` (`approved`\|`rejected`), and `reason` in its detail string (formatted by `ApprovalDecisionRecord`); when a valid `ApprovalDecision` event is recorded for a given `ApprovalRequest` seq, the `ApprovalQueue` projection moves that request from the `pending` list to the `resolved` list, so it no longer appears in `/approval status` output; appended by `/approval approve <seq>` (reason `operator_approved`) and `/approval reject <seq>` (reason `operator_rejected`) — neither command resumes tool execution or emits `ToolCall`/`ToolResult`/`ToolError` |
| `ApprovalResume`             | Emitted by `/approval resume <seq>` immediately after the `SlashCommand` event, before the tool execution sequence (`ToolPolicy → ToolCall → ToolResult\|ToolError`); records that the resume attempt has started and that the `ApprovalResumePlan` is now consumed — even on tool error the plan is not retried and no longer appears in `/approval status`; detail string is formatted by `ApprovalResumeRecord` and carries `request_seq`, `decision_seq`, tool name, and path |

## Mock Run/Turn Flow

Submitting plain text (any input not starting with `/`) is a **deterministic
mock** — it does not call a real LLM. The reply is always
`Mock response for: <text>`, split into one `ModelOutputChunk` event per word.

### Event sequence

When `hello world` is entered, the following events are appended in order:

1. `UserMessage` — the submitted text is recorded (no `SlashCommand`).
2. `RunCreate` — a new Run is created; `run_id` is stored in the event `detail`.
3. `RunStart` — the Run transitions to the running state.
4. `TurnStart` — the first (and only) Turn begins; `turn_id` is in `detail`.
5. `PromptCompile` — the prompt compiler processes the input into the
   System / Project Memory / Conversation / Current User / Workspace Context /
   Operating Rules / Output template; the event `detail` holds the compiled
   prompt preview.
6. `ModelRoute` — `ModelGateway` selects the provider/model/adapter route; the
   event `detail` carries the route metadata (mock provider, model, and adapter).
7. `ModelOutputChunk` × N — one event per word in `Mock response for: <text>`.
8. `AssistantMessage` — the full assembled assistant response is recorded as a trace event.
9. `RunComplete` — the Run finishes successfully.

When a real (or test-injected) adapter returns token-usage metadata, a `ModelUsage` event is
inserted between `AssistantMessage` and `RunComplete`:

```
… ModelOutputChunk × N → AssistantMessage → ModelUsage → RunComplete
```

On the **error path** (`ModelError` / `RunFail`), no `AssistantMessage` event is emitted —
the sequence ends at `ModelError → RunFail` with no `ModelOutputChunk`, `AssistantMessage`,
or `RunComplete`.

### Main panel output

After submitting plain text, the Main panel shows:

```
User: <text>
Assistant: Mock response for: <text>
```

> **Invariant:** The default mock Main panel output (`User: <text>` /
> `Assistant: Mock response for: <text>`) is unchanged by the addition of the
> `AssistantMessage` event — this event is a kernel-level trace and does not affect
> what the TUI renders.

### Persistence

All Run/Turn events are persisted to `.caravan/events.jsonl` exactly like every
other event kind. The `run_id` and `turn_id` values are carried in the event
`detail` string. On restart, these events are reloaded from disk and the Event
Log panel repopulates with the full Run/Turn history from previous sessions.

---

### `/clear` Behaviour

`/clear` empties the **screen log** (the Main panel history) but does **not**
clear the Event Log. The Event Log is append-only; there is no mechanism to
remove events once they have been recorded. `/clear` also does **not** delete
or truncate the on-disk `.caravan/events.jsonl` file — persisted events remain
intact across restarts.

## Mock Runner Boundary

`App::submit()` is responsible only for routing input: it distinguishes slash
commands from plain-text user messages. For plain text it appends the
`UserMessage` event to the log and updates the screen log, then delegates all
Run/Turn event assembly to `crates/kernel/src/runner.rs`.

`runner::run_mock_turn(event_log, message, gateway, manual_tool_context, project_memory)` owns the full Run/Turn lifecycle.
It appends the sequence `RunCreate → RunStart → TurnStart → PromptCompile →
ModelRoute → ModelOutputChunk* → AssistantMessage → RunComplete` to the event log (but **not**
`UserMessage`, which `App::submit()` has already recorded). It returns a `MockRunOutput` value that
the App uses to render the `User:` / `Assistant:` lines in the Main panel.

`App` owns the `ModelGateway` and injects it into the runner on every call.
The runner receives the gateway as a parameter rather than constructing it
internally, keeping the App layer in control of gateway lifecycle and making
the runner independently testable with a supplied gateway instance.

This is a **structural boundary only** — user-visible behaviour is unchanged.
The split ensures that the TUI/App layer and the execution runner remain
independently testable: `runner::run_mock_turn` can be exercised without a
running terminal.

## Prompt Compiler POC

Plain-text user input is compiled into a structured prompt before being passed
to the model. The prompt compiler produces a seven-section template:

```
System:
You are Caravan, a local coding assistant inspired by Claude Code.

Project Memory:
<contents of CLAUDE.md if present at the workspace root, or empty>

Conversation:
No prior conversation context.

Current User:
<message>

Workspace Context:
No external tool context is available.

Operating Rules:
<operating guidelines for the assistant>

Output:
Respond to the current user message.
```

`compile_prompt(message)` renders the empty-history case shown above. It
delegates to `compile_prompt_with_context(message, history, manual_tool_context, project_memory)`, which fills the
`Conversation:` section from recent transcript messages and injects `project_memory`
into the `Project Memory:` section — see
[Prompt Context Window](#prompt-context-window). The result is stored in the
`PromptCompile` event `detail` field as the compiled prompt preview. When you
select a `PromptCompile` event in the **Inspector** panel, the panel displays
this full System / Project Memory / Conversation / Current User / Workspace Context /
Operating Rules / Output preview, letting you inspect exactly what was compiled
for that turn.

## ModelAdapter Boundary

`runner::run_mock_turn` owns the Run/Turn lifecycle and event append — it
appends `RunCreate → RunStart → TurnStart → PromptCompile → ModelRoute →
ModelOutputChunk* → AssistantMessage → RunComplete` — but it no longer contains inline response or token generation
logic. Those responsibilities are delegated to a `ModelAdapter`.

The `ModelAdapter` trait (defined in `crates/kernel/src/model/mod.rs`) exposes a single method:

```rust
fn complete(
    &self,
    context: &ModelAdapterContext,
    request: &ModelRequest,
) -> ModelResult<ModelOutput>;
```

`ModelAdapterContext` carries the resolved routing fields built by the registry from the active `ModelProfile`:

| Field | Type | Description |
|-------|------|-------------|
| `provider` | `ModelProvider` | The selected model provider (e.g. `ModelProvider::Mock`) |
| `model` | `String` | The model identifier string (e.g. `"mock-model"`) |
| `adapter` | `ModelAdapterKind` | The adapter kind (e.g. `ModelAdapterKind::MockModelAdapter`) |

`ModelOutput` carries two fields:

```rust
pub struct ModelOutput {
    pub response: String,
    pub chunks: Vec<String>,
}
```

`chunks` are incremental output fragments (produced by whitespace-splitting the
response text, or future streaming deltas). They are distinct from token counts:
`ModelUsage` (carried separately) holds `prompt_tokens`, `completion_tokens`, and
`total_tokens` from the model API.

`runner::run_mock_turn` no longer calls `MockModelAdapter` directly. Instead it
delegates to `ModelGateway`, which calls `MockModelAdapter.complete` internally
(see [ModelGateway Boundary](#modelgateway-boundary)). The runner iterates
`ModelOutput.chunks` to append one `ModelOutputChunk` event per chunk, and stores
`ModelOutput.response` for the `Assistant:` line in the Main panel.

`MockModelAdapter` is the concrete implementation used in the POC. It produces
a deterministic `"Mock response for: <message>"` response and splits it via
`split_whitespace()` to derive the token list. The mock receives a `&ModelRequest`
and simply leaves `request.prompt` unread.

This is a **structural boundary only** — user-visible behavior and the event
sequence are unchanged. The boundary gives Caravan a clear seam for a real model
adapter while keeping the App layer insulated; because `ModelGateway` today
wraps the concrete `MockModelAdapter`, introducing a real adapter is a localized
`runner.rs`/`crates/kernel/src/model/mod.rs`/gateway wiring change rather than an App-layer change.

## ModelGateway Boundary

`ModelRequest is now defined in` `crates/kernel/src/model/mod.rs` as the shared core adapter request type used by `ModelAdapter`, `ModelAdapterRegistry`, `ModelGateway`, and the runner — no longer a gateway-local type.

`runner::run_mock_turn` obtains model output through
`ModelGateway::complete(ModelRequest) -> Result<ModelResponse, ModelError>` rather than calling a
`ModelAdapter` directly. `ModelGateway` is the central routing layer that sits
between the runner and every concrete adapter:

1. The runner constructs a `ModelRequest` (carrying the compiled prompt and user
   message) and passes it to `ModelGateway::complete`.
2. The gateway selects a route (provider, model, adapter) and records a
   `ModelRoute` event carrying that route metadata.
3. The gateway delegates to the selected adapter — currently always
   `MockModelAdapter` — and returns the `ModelResponse` to the runner.

The gateway is where future multi-model routing, model selection, fallback
logic, cost tracking, and provider configuration will live. New adapter
integrations require changes only inside `ModelGateway` and `crates/kernel/src/model/mod.rs`,
leaving the runner and App layer untouched.

### Model Config Stub

`ModelGateway` owns a `ModelConfig` that carries the active routing
configuration. `ModelConfig` holds an `active_profile` field whose type is
`ModelProfile`. Each `ModelProfile` contains three fields:

| Field      | Description                                                                     |
|------------|---------------------------------------------------------------------------------|
| `provider` | The model provider; typed as `ModelProvider` (e.g. `ModelProvider::Mock`)       |
| `model`    | The model identifier string (e.g. `"mock-model"`)                               |
| `adapter`  | The adapter kind; typed as `ModelAdapterKind` (e.g. `ModelAdapterKind::MockModelAdapter`) |

The default profile is:

```
provider  = mock
model     = mock-model
adapter   = MockModelAdapter
```

The `ModelRoute` detail recorded in the event log is built directly from this
active profile — the gateway reads `active_profile.provider`, `.model`, and
`.adapter` and uses them to construct the route metadata emitted in the
`ModelRoute` event.

`App` owns the `ModelGateway` (constructed at startup with the default
`ModelConfig`/`ModelProfile`) and injects it into
`runner::run_mock_turn(event_log, message, gateway, manual_tool_context, project_memory)` on every call.

> **This is NOT a real LLM integration.** There is no API key, no provider
> configuration, no network call, and no external service dependency. The
> gateway still wraps `MockModelAdapter` and produces the same deterministic
> `"Mock response for: <message>"` output as before.
>
> This is a mock stub, not a real LLM API integration. The `ModelConfig` and
> `ModelProfile` structures exist solely to establish the routing seam; no
> connection to any external model provider is made.

## Model Adapter Registry Stub

`ModelGateway` no longer constructs `MockModelAdapter` directly. Instead, it
owns a `ModelAdapterRegistry` and delegates every completion call to
`ModelAdapterRegistry::complete(profile, request) -> Result<ModelOutput, ModelError>`. The registry
selects the adapter by matching on the typed `ModelAdapterKind` from the
`ModelProfile`; the single supported arm is `ModelAdapterKind::MockModelAdapter`,
which delegates to the owned `MockModelAdapter` instance and preserves the same
deterministic mock path.

> **This is still a mock stub.** The `ModelAdapterRegistry` performs no model
> switching, fallback, or runtime reconfiguration. There is no real LLM, no API
> key, no config file lookup, and no network call. The registry seam exists to
> isolate adapter construction from the gateway and to make future real-adapter
> registration possible without changing `ModelGateway`.

## Provider / Adapter Type Cleanup POC

`provider` and `adapter` in `ModelProfile` are now small typed enums rather than
plain strings. Both are defined in `crates/kernel/src/model_types.rs`:

- **`ModelProvider`** — variants: `Mock`, `OpenAI`. Exposes `as_str()` returning `"mock"` and `"openai"` respectively.
- **`ModelAdapterKind`** — variants: `MockModelAdapter`, `OpenAICompatibleAdapter`. Exposes `as_str()` returning
  `"MockModelAdapter"` and `"OpenAICompatibleAdapter"` respectively.

**Provider vs. adapter distinction:** `provider` is the user-selected vendor name (e.g. `openai`) that
appears in `ModelRoute` events and config keys. `adapter` is the internal protocol implementation
(e.g. `OpenAICompatibleAdapter`) that handles the actual request/response translation. The adapter name
deliberately remains `OpenAICompatibleAdapter` — it identifies the protocol, not the vendor.

The `model` field remains a `String` (e.g. `"mock-model"`).

The `ModelRoute` event detail emitted by `ModelGateway` is unchanged:

```
provider=mock model=mock-model adapter=MockModelAdapter
```

`as_str()` on each enum produces those values, so the observable event output is
identical to the plain-string era.

> **This is type-tidying only.** There is no real provider selection, model
> switching, or API integration. The enum variants (`Mock`, `MockModelAdapter`)
> still drive the same deterministic mock path as before.

## Model Error Boundary

`ModelAdapter::complete`, `ModelAdapterRegistry::complete`, and
`ModelGateway::complete` now return `Result<_, ModelError>` rather than a bare
`ModelOutput`/`ModelResponse`. This gives the runner a typed error boundary to
handle model-layer failures.

When `ModelGateway::complete` returns an `Err(ModelError)`, the runner:

1. Appends a `ModelError` event carrying the `kind=... message="..."` detail.
2. Appends a `RunFail` event to signal that the Run did not complete successfully.
3. Does **not** append any `ModelOutputChunk` or `RunComplete` events.

The failure path is exercised only via the `#[cfg(test)]` helper
`ModelGateway::failing_for_test`, which constructs a gateway wired to always
return an error. There is no user-facing command or runtime configuration that
triggers this path during normal application use.

> **This is still a mock/test-only error boundary.** It does NOT implement real
> LLM API or network failure handling. `MockModelAdapter` still always succeeds
> with the same deterministic `"Mock response for: <message>"` response and
> token list. The `Result` return type and `failing_for_test` helper exist solely
> to establish a typed seam for future real-adapter error propagation.

## OpenAI-compatible Adapter Skeleton

`OpenAICompatibleAdapter` lives in `crates/kernel/src/model/openai/compatible.rs` and
implements the `ModelAdapter` trait. Its `complete()` method makes **no real
network call** and always returns `Err(ModelError::AdapterFailure)`. This is a
skeleton only — there is no real API or network integration of any kind.

Two new typed variants have been added to the enums in `crates/kernel/src/model_types.rs`:

| Enum | Variant | `as_str()` value |
|------|---------|-----------------|
| `ModelProvider` | `OpenAI` | `"openai"` |
| `ModelAdapterKind` | `OpenAICompatibleAdapter` | `"OpenAICompatibleAdapter"` |

`ModelAdapterRegistry` owns the `OpenAICompatibleAdapter` instance as a normal
field (alongside `MockModelAdapter`). Adapter selection is driven by matching on
`ModelAdapterKind` inside the registry — `ModelGateway` delegates to the
registry as before and is unaware of the concrete adapter type.

`OpenAI` profiles (i.e. `ModelProfile` values whose `adapter` field is
`ModelAdapterKind::OpenAICompatibleAdapter`) are constructed **only in tests**.
The adapter itself is always constructed by `ModelAdapterRegistry`; nothing
outside the registry instantiates `OpenAICompatibleAdapter` directly.

The **default** configuration is unchanged and still routes every real run to
`MockModelAdapter`:

```
provider=mock model=mock-model adapter=MockModelAdapter
```

> **This is a skeleton with no real API/network integration.** `OpenAICompatibleAdapter::complete()`
> performs no network call and always returns `ModelError::AdapterFailure`. The
> new enum variants and registry wiring exist solely to establish the structural
> seam; no connection to any OpenAI-compatible endpoint is made at runtime.

## OpenAI-compatible Request / Response Payload Types

`crates/kernel/src/model/openai/types.rs` defines the five serde-serializable structs that
describe the JSON body for OpenAI-compatible chat completions endpoints:

| Struct | Role |
|--------|------|
| `OpenAIChatRequest` | Top-level request body (`model`, `messages`, `stream`) |
| `OpenAIChatMessage` | Individual message (`role`, `content`) |
| `OpenAIChatResponse` | Top-level response body (`choices`, optional `usage`) |
| `OpenAIChatChoice` | Single choice wrapping a `message` |
| `OpenAIUsage` | Token counts (`prompt_tokens`, `completion_tokens`, `total_tokens`) |

All five structs derive `Serialize` and `Deserialize` (via `serde`).

### `OpenAIChatRequest::from_model_request`

Converts a `ModelRequest` into an `OpenAIChatRequest`:

- `ModelRequest.prompt` is carried verbatim as the `content` of a single `"user"` message.
- `stream` is always set to `false`.
- The target model name is passed separately and stored in the `model` field.

### `OpenAIChatResponse::first_assistant_content` and `to_model_output`

- `first_assistant_content` returns the `content` string of the first choice, or
  `None` if `choices` is empty.
- `to_model_output` converts the response into a `ModelOutput`:
  - The response text is taken verbatim from `first_assistant_content`.
  - Output chunks are produced by splitting the response on whitespace (`split_whitespace`).
  - If `choices` is empty (no assistant content), `to_model_output` returns
    `Err(ModelError::AdapterFailure)`.

### What did NOT change

The default mock flow (`provider=mock model=mock-model adapter=MockModelAdapter`) is untouched.

`OpenAICompatibleAdapter::complete()` now uses the payload types: it builds an
`OpenAIRequestPlan` via `OpenAIRequestBuilder::build(&self.config, &context.model, request)`,
then passes the plan to `StubOpenAIHttpClient::send_chat_completion`. The stub always returns
an error, which the adapter maps to `ModelError::AdapterFailure` carrying the exact message:

```
OpenAI-compatible HTTP client is a skeleton in this POC
```

No async runtime, real HTTP call, API key read, or network dependency was added.
The `#![allow(dead_code)]` attribute on the payload-types module has been removed now
that `complete()` builds real request plans from those structs.

## OpenAI-compatible Adapter Config Stub

`OpenAICompatibleConfig` in `crates/kernel/src/model/openai/config.rs` is the configuration
boundary for the OpenAI-compatible adapter. It carries three fields:

| Field | Default | Meaning |
|-------|---------|---------|
| `base_url` | `https://api.openai.com/v1` | Root URL of the target OpenAI-compatible endpoint |
| `api_key_env` | `OPENAI_API_KEY` | Name of the environment variable that holds the API key — the variable name only, never the key value itself |
| `timeout_secs` | `30` | Request timeout in seconds (reserved for future use) |

The `chat_completions_url()` helper returns `base_url` joined with
`/chat/completions`, trimming any trailing slash from `base_url` before
appending so the path is never doubled (e.g. `https://api.openai.com/v1/chat/completions`).

`OpenAICompatibleAdapter` now owns this config. It is constructed via:

- `OpenAICompatibleAdapter::new(config)` — accepts an explicit `OpenAICompatibleConfig`.
- `OpenAICompatibleAdapter::config()` — returns a shared reference to the stored config.
- `OpenAICompatibleAdapter::default()` — delegates to `OpenAICompatibleConfig::default()`,
  producing the standard OpenAI endpoint / `OPENAI_API_KEY` / 30 s configuration.

The default application flow is unchanged: every real run still routes to
`MockModelAdapter` (`provider=mock model=mock-model adapter=MockModelAdapter`).
`OpenAI` profiles are constructed only in tests.

> **This is configuration boundary preparation only — NOT a real API integration.**
> No environment variable is read, no API key is loaded, no HTTP client exists,
> and no network call is made. `OpenAICompatibleAdapter::complete()` still always
> returns `Err(ModelError::AdapterFailure)` with the same skeleton message. The
> config struct and `chat_completions_url()` helper exist solely to establish the
> typed configuration seam that a future real adapter implementation will use.

## OpenAI-compatible Request Builder

`OpenAIRequestBuilder::build` in `crates/kernel/src/model/openai/request.rs` combines an `OpenAICompatibleConfig`,
a model name (`&str`), and a `ModelRequest` into an `OpenAIRequestPlan` — a request plan
describing what *would* be sent to the OpenAI-compatible endpoint if a real HTTP client
existed. Producing an `OpenAIRequestPlan` is not an API call; no network connection is
opened and no credentials are touched.

### `OpenAIRequestPlan` Fields

| Field | Type | Meaning |
|-------|------|---------|
| `url` | `String` | The fully-resolved chat-completions URL (from `OpenAICompatibleConfig::chat_completions_url()`) |
| `api_key_env` | `String` | The **name** of the environment variable that would hold the API key — never the key value itself; `std::env::var` is never called |
| `timeout_secs` | `u64` | Request timeout in seconds, copied from the config |
| `body` | `OpenAIChatRequest` | The serialisable request body, containing the model name and the messages list built from `ModelRequest` |

`OpenAIRequestBuilder::build(config, model, request)` returns `OpenAIRequestPlan` by value. It
reads fields from the config and maps `ModelRequest.prompt` to a single user message
(`role: "user"`). No I/O of any kind is performed.

> **This is a request-plan builder only — NOT a real HTTP integration.**
> No HTTP call is performed. `std::env::var` is never called — `api_key_env` is
> copied as a plain string and the actual API key is never read or resolved. No
> `Authorization header` is constructed. The builder itself performs no HTTP call
> and never resolves keys; no direct `tokio` or `hyper` dependency exists in the
> workspace. The kernel crate carries `reqwest` solely for `BlockingOpenAIHttpClient`
> (the opt-in blocking client, not used here). `OpenAICompatibleAdapter::complete()`
> still always returns `Err(ModelError::AdapterFailure)` with the same skeleton error
> message. The plan struct exists solely to make the intended request shape
> inspectable and testable without performing any I/O.

## Model Runtime Config Source

The Model Runtime Config Source is a configuration assembly step, not a real API call layer.

`ModelRuntimeConfig` is the top-level configuration type that combines `ModelConfig` (provider
selection and model name) with `OpenAICompatibleConfig` (base URL, API-key env-var name, and
timeout). It is constructed through `from_env_map`, which accepts a pure key/value map — it
never reads process environment variables and never reads API key values.
`CARAVAN_OPENAI_API_KEY_ENV` carries only the **name** of the environment variable; the actual
key value is never loaded or resolved at this stage.

### Supported Keys and Defaults

| Key | Values / Default | Notes |
|-----|-----------------|-------|
| `CARAVAN_MODEL_PROVIDER` | `mock` \| `openai`, default `mock` | Selects the active model adapter |
| `CARAVAN_MODEL` | default `mock-model` (mock) / `openai-model` (openai) | Model name passed to the adapter |
| `CARAVAN_OPENAI_BASE_URL` | default `https://api.openai.com/v1` | Base URL for OpenAI-compatible endpoints |
| `CARAVAN_OPENAI_API_KEY_ENV` | default `OPENAI_API_KEY` | Name of the env var that holds the API key — the key value itself is never read here |
| `CARAVAN_OPENAI_TIMEOUT_SECS` | default `30` | Request timeout in seconds; `0` or a non-numeric value is an error |
| `CARAVAN_OPENAI_HTTP_CLIENT` | `stub` \| `blocking`, default `stub` | Selects the HTTP client implementation; `blocking` opts in to the real reqwest-based client; any other value is a config error |

### Default Flow

The default user flow is unchanged: the mock provider is selected, and `ModelRoute` logs the
detail `provider=mock model=mock-model adapter=MockModelAdapter`. No OpenAI-compatible config is
consulted during a normal run.

> **This is a configuration assembly step only — NOT a real API integration.**
> `from_env_map` never calls `std::env::var`, never loads an API key, and performs no I/O of
> any kind. It exists solely to translate a flat key/value map into the typed config structs
> consumed by the model adapter selection logic.

## Model Runtime Config Gateway Wiring

The Model Runtime Config Gateway Wiring stage connects the typed configuration produced by
`ModelRuntimeConfig` to the `ModelGateway` construction boundary — still without performing
any real API integration.

`ModelGateway::from_runtime_config(ModelRuntimeConfig)` wires the configuration into the
gateway in two steps:

1. `runtime_config.model_config` is used as the gateway routing config, selecting which
   model adapter handles requests.
2. `runtime_config.openai_config` and `runtime_config.openai_http_client_kind` are passed
   through `ModelAdapterRegistry::from_openai_runtime` into the `OpenAICompatibleAdapter`,
   making the typed config available at the adapter construction boundary and selecting the
   stub or blocking HTTP client implementation.

`ModelGateway::default()` and the main App flow are unchanged — the default path continues
to use the mock adapter exclusively, and no OpenAI-compatible logic is executed during a
normal run.

> **This is construction wiring only — NOT a real API integration.**
> No environment variables are read at this stage. No API key value is read or resolved.
> On the default runtime-config path, Caravan wires the stub HTTP client; the real blocking HTTP client is not constructed, and no network call
> of any kind is made. The `OpenAICompatibleAdapter` still returns a stub error on every
> invocation. The default user flow continues to use the mock adapter and is wholly
> unaffected by this wiring.

## App Runtime Config Bootstrap

The App Runtime Config Bootstrap stage wires the process environment into the application
at startup — still without performing any real API call.

### Environment Keys

| Key | Effect |
|-----|--------|
| `CARAVAN_MODEL_PROVIDER` | Selects the active model provider (`mock` or `openai`); defaults to `mock` |
| `CARAVAN_MODEL` | Model name passed to the selected adapter; provider-specific default applied if absent |
| `CARAVAN_OPENAI_BASE_URL` | Base URL for OpenAI-compatible endpoints; defaults to `https://api.openai.com/v1` |
| `CARAVAN_OPENAI_API_KEY_ENV` | Holds the **name** of the env var that would contain the API key (e.g. `OPENAI_API_KEY`) — the actual key value is never read in this POC |
| `CARAVAN_OPENAI_TIMEOUT_SECS` | Request timeout in seconds; defaults to `30` |
| `CARAVAN_OPENAI_HTTP_CLIENT` | Selects the HTTP client implementation (`stub` or `blocking`); defaults to `stub`; `blocking` opts in to the real reqwest-based client; any other value exits with a config error |

### Bootstrap Flow

```
process env → ModelRuntimeConfig::from_process_env() → ModelGateway::from_runtime_config() → App::with_store_and_gateway(...)
```

`ModelRuntimeConfig::from_process_env()` reads the six `CARAVAN_*` keys from the real
process environment and builds the typed config. The resulting `ModelRuntimeConfig` is then
passed to `ModelGateway::from_runtime_config()` to wire the gateway, which is handed to
`App::with_store_and_gateway(...)` before the TUI starts.

### Config-Error Behavior

Invalid values — such as an unrecognised provider name, a non-numeric timeout, or an
unrecognised HTTP client name — cause the bootstrap to print a human-readable error message
to stderr and exit with status 1 before the TUI is initialised. The error includes the config
error kind and the invalid value, such as `kind=unknown_provider message="unknown model provider: <value>"`,
`kind=invalid_timeout message="invalid timeout seconds: <value>"`, or
`kind=unknown_openai_http_client message="unknown OpenAI HTTP client: <value>"`.

### Usage Example

```sh
CARAVAN_MODEL_PROVIDER=openai cargo run
```

The app starts normally. A user message reaches the skeleton `ModelError`/`RunFail` flow
inside `OpenAICompatibleAdapter` with no network call made; no API key value is read.
The `ModelRoute` event detail for this path is:

```
provider=openai model=openai-model adapter=OpenAICompatibleAdapter
```

(`provider=openai` is the user-selected vendor name; `adapter=OpenAICompatibleAdapter` is
the internal protocol implementation — it identifies the wire protocol, not the vendor.)

> **This is config bootstrap only — NOT a real API integration.**
> `from_process_env()` reads only the six `CARAVAN_*` keys; no API key value is ever read
> or resolved at bootstrap time. On the default App path (`CARAVAN_OPENAI_HTTP_CLIENT=stub`
> or absent), Caravan wires the stub HTTP client; the real blocking HTTP client is not constructed, and no network call of any kind is made.
> Real network calls are possible **only** when both `CARAVAN_MODEL_PROVIDER=openai`
> and `CARAVAN_OPENAI_HTTP_CLIENT=blocking` are set explicitly; setting
> `CARAVAN_MODEL_PROVIDER=openai` alone (without `CARAVAN_OPENAI_HTTP_CLIENT=blocking`)
> still fails with the skeleton error from `StubOpenAIHttpClient`.

### Blocking HTTP Client Opt-In

To route requests through the real reqwest-based HTTP client, set **both** of the following
environment variables before starting the app:

```sh
CARAVAN_MODEL_PROVIDER=openai
CARAVAN_OPENAI_HTTP_CLIENT=blocking
```

When `blocking` is selected, the API key is read **at send time** from the environment
variable whose **name** is stored in `CARAVAN_OPENAI_API_KEY_ENV` (default `OPENAI_API_KEY`).
If that env var is absent or empty when a request is sent, the adapter returns a
`MissingApiKey` model error that contains only the env var **name** — the key value itself
is never written to events, logs, or errors.

> **Security note:** The API key value is never logged, stored in config structs, or included
> in any error message. Refer to the key only by the env var name (`CARAVAN_OPENAI_API_KEY_ENV`).

## OpenAI HTTP Client Boundary Skeleton

`crates/kernel/src/model/openai/http.rs` defines the synchronous HTTP client boundary for
OpenAI-compatible endpoints. It exposes five public items:

| Item | Kind | Description |
|------|------|-------------|
| `OpenAIHttpClient` | trait | The client boundary — one method, no async |
| `OpenAIHttpError` | enum | Five-variant error type: `NotImplemented`, `MissingApiKey`, `RequestFailure`, `HttpStatus`, `ResponseDecode` |
| `OpenAIHttpResult<T>` | type alias | `Result<T, OpenAIHttpError>` |
| `StubOpenAIHttpClient` | struct | Stub implementation; performs no network I/O; always returns `OpenAIHttpError::NotImplemented` |
| `BlockingOpenAIHttpClient` | struct | Opt-in real implementation; reqwest blocking, Bearer auth from env var named by `api_key_env`, per-plan timeout; **not wired into the default App path** |

### Client Boundary

`OpenAIHttpClient` declares a single synchronous method:

```
OpenAIHttpClient::send_chat_completion(&OpenAIRequestPlan) -> OpenAIHttpResult<OpenAIChatResponse>
```

The method receives a fully-built `OpenAIRequestPlan` whose fields — `url`, `api_key_env`,
`timeout_secs`, and `body` — reach the boundary intact. The trait is synchronous by design.

### Stub Implementation

`StubOpenAIHttpClient` is the default implementation injected on the App path. It performs
**no network I/O** and always returns:

```
Err(OpenAIHttpError::NotImplemented {
    message: "OpenAI-compatible HTTP client is a skeleton in this POC"
})
```

### Adapter Wiring

`OpenAICompatibleAdapter` IS now wired to the `OpenAIHttpClient` boundary. The default
production client is `StubOpenAIHttpClient`.

The failure boundary moved one level down:

| Before | After |
|--------|-------|
| Adapter-level skeleton: `OpenAICompatibleAdapter::complete()` returned a hard-coded error directly | HTTP-client-level skeleton: `StubOpenAIHttpClient::send_chat_completion` returns the error; the adapter propagates it |

The old adapter-level skeleton message (`"OpenAI-compatible adapter is a skeleton in this POC"`)
is no longer produced anywhere on the production path. The error surface is now
`"OpenAI-compatible HTTP client is a skeleton in this POC"` — emitted by
`StubOpenAIHttpClient` and mapped by the adapter to `ModelError::AdapterFailure`.

No API key value is read and no `Authorization` header is built at any point in the default flow.

### BlockingOpenAIHttpClient

`BlockingOpenAIHttpClient` is the opt-in real implementation. It uses `reqwest` blocking,
sends a Bearer auth header populated from the env var named by `api_key_env`, and applies
the per-plan `timeout_secs`. Typed errors cover missing key (`MissingApiKey`), request
failure (`RequestFailure`), non-2xx HTTP status (`HttpStatus`), and JSON decode failure
(`ResponseDecode`).

`BlockingOpenAIHttpClient` is **not wired into the default App path**:
`OpenAICompatibleAdapter::new/default` and the runtime-config path still inject
`StubOpenAIHttpClient`. As a result, `CARAVAN_MODEL_PROVIDER=openai` still
returns the exact skeleton error:

```
OpenAI-compatible HTTP client is a skeleton in this POC
```

emitted as `ModelError::AdapterFailure` / `RunFail`, with no `ModelOutputChunk` or `RunComplete`
event. The default mock flow is byte-identical.

## OpenAI HTTP Client Injection Boundary

This subsection documents the dependency-injection seam added on top of the skeleton above.
The injection boundary is a preparation step for swapping in a real HTTP client in a future
iteration — it performs **no real network calls**, reads **no API keys**, and adds **no
network or runtime dependencies**.

### Adapter-Level Injection (`with_http_client`)

`OpenAICompatibleAdapter` now holds a `Box<dyn OpenAIHttpClient>` field instead of
hard-coding the stub internally. Injection is done via the secondary constructor:

```
OpenAICompatibleAdapter::with_http_client(config, client: Box<dyn OpenAIHttpClient>) -> Self
```

The primary constructor (`OpenAICompatibleAdapter::new`) continues to use
`StubOpenAIHttpClient` as the default, so the production path is unchanged:

```
OpenAICompatibleAdapter::new(config)
  └─ delegates to with_http_client(config, Box::new(StubOpenAIHttpClient::default()))
```

### Registry-Level Injection (`with_openai_http_client`)

`ModelAdapterRegistry` exposes a secondary constructor that carries the injected client
through to the adapter:

```
ModelAdapterRegistry::with_openai_http_client(openai_config, client: Box<dyn OpenAIHttpClient>) -> Self
```

The primary constructor (`ModelAdapterRegistry::new`) continues to supply
`StubOpenAIHttpClient`, keeping the default production path intact.

### Default-Path Behavior (unchanged)

When the production path is used, `StubOpenAIHttpClient` is the active implementation and
every call to `send_chat_completion` returns:

```
Err(OpenAIHttpError::NotImplemented {
    message: "OpenAI-compatible HTTP client is a skeleton in this POC"
})
```

The adapter maps this to `ModelError::AdapterFailure` — the same surface as before T-4.

### Test-Injection Pattern

`ModelGateway::with_openai_http_client_for_test` accepts an arbitrary
`Box<dyn OpenAIHttpClient>` and wires it through the registry and adapter. Tests supply a
fake success client that returns a valid `OpenAIChatResponse`, allowing the
`OpenAIChatResponse → ModelOutput` conversion path to be exercised in isolation without
any network I/O.

> **This is a seam for a future real client — NOT a real integration.** No real network
> calls are made, no API key is read, no `Authorization` header is constructed, and no
> HTTP or async dependency is introduced. Swapping in a real HTTP client is explicitly
> deferred to a future task.

## Model Usage Metadata Boundary

This section documents the token-usage metadata path added on top of the OpenAI HTTP client
injection boundary. When a real (or test-injected) HTTP client returns token counts,
the runner captures them as a `ModelUsage` event. When the mock adapter is used, the path
is a no-op — no event is emitted and no existing behavior changes.

### `ModelUsage` Type

`ModelUsage` is a plain value type defined in `crates/kernel/src/model/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}
```

It deliberately derives no serde traits. Usage data lives only in the event log's detail
string; the struct itself is never serialized to or deserialized from JSON.

`ModelOutput` and `ModelResponse` each carry `usage: Option<ModelUsage>`. `None` means
the adapter did not supply usage information; `Some` carries the three token counts.

### Conversion Point (`OpenAIChatResponse::to_model_output`)

`OpenAIChatResponse::to_model_output()` maps `OpenAIUsage` to `ModelUsage` field-by-field:

```
OpenAIUsage { prompt_tokens, completion_tokens, total_tokens }
    → ModelUsage { prompt_tokens, completion_tokens, total_tokens }
```

The mapping is 1:1 with no rounding, clamping, or transformation. If the HTTP response
carries `"usage": null` or omits the field entirely, `output.usage` is `None`.

`MockModelAdapter::complete()` always returns `usage: None` — the mock adapter has no
token-count concept and the default flow is unchanged.

### Runner Event Emission

The runner in `crates/kernel/src/runner.rs` emits a `ModelUsage` event only when
`response.usage` is `Some`. The event is appended **after** `AssistantMessage` and **immediately before** `RunComplete`:

```
… ModelOutputChunk (×N) → AssistantMessage → ModelUsage → RunComplete
```

The detail string uses the exact format:

```
prompt_tokens=10 completion_tokens=5 total_tokens=15
```

When `usage` is `None` (the default mock flow), no `ModelUsage` event is emitted and the
sequence remains `… ModelOutputChunk (×N) → AssistantMessage → RunComplete`. Error paths
(`ModelError` / `RunFail`) emit neither `ModelOutputChunk`, `AssistantMessage`, `ModelUsage`,
nor `RunComplete`.

### Forward-Compatibility with Stored Events

`ModelUsage` events are written to `.caravan/events.jsonl` as ordinary JSONL lines. A
binary built before this boundary was introduced will encounter these lines on restore and
skip them via the existing malformed/unknown-line skip behavior in `storage.rs` — the same
path already used for any line that cannot be deserialized into a known `EventKind`. This
is intentional: no migration, no schema versioning, and no changes to the persistence layer
are required.

### Non-Goals

This boundary explicitly does not include:

- Pricing, cost tracking, or billing calculation.
- Streaming usage (the `stream` flag is always `false`).
- Retry logic or partial-usage recovery on error.
- Async or `tokio` dependencies (the client boundary remains synchronous).
- New crate dependencies of any kind.
- Tests against a real OpenAI API endpoint.

> **This is usage metadata capture only — NOT a billing or analytics feature.** Token counts
> are surfaced in the event log for observability during development. No pricing data is
> computed, no usage is reported to any external service, and no new runtime dependencies
> are introduced.

## ConversationTranscript Projection

`ConversationTranscript` is a read-only, kernel-only projection that reconstructs the
User/Assistant conversation from the event log.

### How it works

- **Source events:** Only `UserMessage` and `AssistantMessage` events are used. All other
  event kinds (`RunCreate`, `ModelOutputChunk`, `RunComplete`, etc.) are ignored.
- **Ordering:** Events are traversed in ascending `seq` order, preserving the original
  submission order.
- **Output shape:** Each entry in the transcript carries a role (`User` or `Assistant`) and
  the message text extracted from the event `detail` field.

### Read-only contract

`ConversationTranscript` is a pure read projection — it performs **no mutation**:

- It does **not** append, modify, or remove any event from the in-memory `EventLog`.
- It does **not** write to or truncate `.caravan/events.jsonl`.
- It does **not** trigger any side effect in the storage layer.

### Wired into prompt compilation

`ConversationTranscript` is now wired into prompt compilation through the
[Prompt Context Window](#prompt-context-window): the runner projects the
transcript and feeds recent `User`/`Assistant` messages into the compiled
prompt as conversation history. The projection remains a pure read — wiring it
into the prompt does not mutate the event log or `.caravan/events.jsonl`.

## Prompt Context Window

The Prompt Context Window is the minimal first step of putting conversation
history into the compiled prompt. Before each turn, the runner projects the
`ConversationTranscript` from the event log, drops the current user message
(which the TUI appends before the runner runs), and renders the remaining
recent messages into the prompt's `Conversation:` section. The current message
appears only under `Current User:` and is never duplicated into `Conversation:`.

### What it does

- **Source:** only `UserMessage` and `AssistantMessage` events — slash commands
  and trace events (`ModelRoute`, `ModelOutputChunk`, `ModelUsage`, …) are never
  included.
- **Window:** capped at the last `DEFAULT_PROMPT_HISTORY_MESSAGES` (6) messages.
  When there is no prior history, the section reads `No prior conversation
  context.`.
- **Helper:** `compile_prompt_with_context(current_user_message, history, manual_tool_context, project_memory)` owns
  both the formatting and the window cap; `compile_prompt(message)` is its
  empty-history case.
- **`/clear`:** clears only the on-screen log, not the event log, so it is
  **not** a conversation boundary — history from before a `/clear` still appears
  in later prompts.
- **OpenAI opt-in:** on the OpenAI-compatible path the compiled, context-bearing
  prompt is exactly what is sent to the model. No API key value is ever written
  to the event log, logs, or this README.

### What it is NOT

This is **not** long-term memory. There is no vector search, retrieval,
embedding, summarization, conversation compression, token counting, or
model-specific context-window math — only a fixed, recent, in-session window.

## Event Persistence

Events are appended to `.caravan/events.jsonl` as JSONL (one JSON object per
line). The file is created automatically on first run if it does not exist.

- **On startup** — existing events are loaded from the file so the Event Log
  panel repopulates with events from previous runs.
- **Sequence numbering** — new events continue from the last stored `seq + 1`,
  ensuring sequence numbers are globally unique across restarts.
- **Append-only** — the file is never truncated or rewritten. `/clear` does
  not delete or modify `.caravan/events.jsonl`.

### Restart Verification

Manual steps to confirm persistence is working:

1. Run `cargo run` from the project root.
2. Enter one or more commands (e.g. type `hello` and press Enter).
3. Type `/exit` to close the application.
4. Run `cargo run` again.
5. Confirm that events from the previous run reappear in the **Event Log**
   panel, and that the `seq` of new events continues from where the last
   run left off.

> **Note:** `.caravan/` is created relative to the working directory at
> launch. Run `cargo run` from the project root to ensure the directory is
> placed consistently.

> **Stale event data:** If the Event Log shows unexpected or missing entries
> after updating the application (e.g. following an `EventKind` rename), the
> on-disk `.caravan/events.jsonl` may contain events written with old variant
> names that are silently skipped on load. Delete the directory before running
> to start fresh:
>
> ```sh
> rm -rf .caravan
> ```

## Read-only Tool Harness Skeleton

`crates/kernel/src/tool/registry.rs` provides the kernel `ToolRegistry` with
**read-only** workspace inspection tools. All paths are confined to the
workspace root before any filesystem operation is attempted. The tools are
surfaced to the user via the `/tool list` and `/tool read` slash commands, and
every execution is traced into the EventLog by `ToolEventRunner`.
Tool results are not yet injected into the model prompt.
There is no model-driven tool calling yet.

### ToolRegistry

`ToolRegistry::new_readonly()` constructs a registry pre-populated with the
two read-only tools below. All tools share a single workspace root, and every
path operation is confined to that root before the tool logic runs.

### Registered Tools

| Tool | Behaviour |
|------|-----------|
| `list_files` | Lists the immediate children of a directory (non-recursive), returned in sorted order. |
| `read_file` | Reads a file as UTF-8 text; capped at 64 KiB. Returns an error if the file exceeds the limit or is not valid UTF-8. |

### Workspace-Root Path Confinement

Every path supplied to a tool is validated against the workspace root before
any filesystem operation is attempted. The following path forms are rejected:

- **Absolute paths** — only paths relative to the workspace root are accepted.
- **Parent-directory traversals** (`../`) — any path that would escape the root
  via `..` components is rejected.
- **Symlink escapes** — resolved symlinks that point outside the workspace root
  are rejected.

Rejection returns an error to the caller; no filesystem operation is performed
on the offending path.

### ToolEventRunner — Tracing Boundary (kernel-only)

`ToolEventRunner` wraps `ToolRegistry` and traces each read-only tool execution
into the `EventLog` as a sequence of typed events:

| Event | When it is recorded |
|-------|---------------------|
| `ToolPolicy` | Immediately before `ToolCall`; carries the policy decision for the request (see [Read-only Tool Policy / Approval-lite Boundary](#read-only-tool-policy--approval-lite-boundary)) |
| `ToolCall` | After the `ToolPolicy` allow decision and immediately before the tool executes; carries the tool name and input arguments |
| `ToolResult` | After a successful execution; carries a **summary only** — never the full file content |
| `ToolError` | After a failed execution; carries the tool name and error detail |

`ToolResult` stores only a short human-readable summary of the outcome (e.g. a
file-listing count or a byte-length notice), never the raw file content returned
by `read_file` or `list_files`. This keeps event-log detail strings bounded in
size regardless of the files being read.

> `ToolEventRunner` is invoked by the `/tool list` and `/tool read` slash
> commands (via `App::submit()`). It appends a `ToolPolicy` event, then
> `ToolCall`, then `ToolResult` or `ToolError` to the EventLog; those events
> render in the Inspector under the `Tool Policy`, `Tool Call`, `Tool Result`,
> and `Tool Error` labels. No model output
> triggers a tool call — tool results are not yet injected into the model
> prompt, and there is no model-driven tool calling yet.

## Read-only Tool Policy / Approval-lite Boundary

`ToolPolicyEngine` provides the policy gate between a tool request and its
execution. It evaluates every `ToolRequest` before any filesystem operation
is attempted and records the outcome as a `ToolPolicy` event in the `EventLog`.

### Current Policy: Auto-allow Read-only Tools

`ToolPolicyEngine::read_only()` is the engine wired into `ToolEventRunner`.
Its current policy is to **auto-allow every read-only tool** — no interactive
approval is required. The `ToolPolicy` event is always `decision=allow` on the
production path.

### `ToolPolicy` Event Detail Format

The `ToolPolicy` event `detail` string uses the following format:

```
tool=<name> path="<path>" risk=read_only decision=allow reason=read_only_auto_allow
```

For example, reading `README.md` produces:

```
tool=read_file path="README.md" risk=read_only decision=allow reason=read_only_auto_allow
```

### Updated Event Sequences

**Success sequence** (tool executes without error):

```
SlashCommand, ToolPolicy, ToolCall, ToolResult
```

**Workspace-violation sequence** (path rejected by the workspace confinement
check — the policy still allows, but the registry rejects the path after
`ToolCall` is recorded):

```
SlashCommand, ToolPolicy, ToolCall, ToolError
```

The `ToolPolicy` event records the policy decision **before** the registry
validates the path; a workspace violation is therefore a post-policy registry
error, not a policy denial.

### What Does NOT Emit `ToolPolicy`

- **`ModelToolRequest` detection** — detecting a `CARAVAN_TOOL_REQUEST` block
  in a model response records only a `ModelToolRequest` event. No tool is
  executed and no `ToolPolicy` event is emitted.
- **The basic mock flow** — plain text submitted to the mock runner produces
  the standard `UserMessage → RunCreate → … → RunComplete` sequence with no
  `ToolPolicy` event, because no tool is executed.

### Not Yet Implemented

The following are **explicitly out of scope** for this POC and are not
implemented:

- **Approval modal** — no interactive confirmation UI exists; `ToolPolicyDecision::Deny`
  is exercised by tests only.
- **Sandbox** — no sandboxing or capability-restriction layer is applied to
  tool execution.
- **Write, shell, and network tools** — only the two read-only tools
  (`list_files`, `read_file`) are registered; no write, shell, delete, or
  network tool exists in this stage.

## Approval Gate Skeleton

`crates/kernel/src/approval.rs` defines the data types that form the Approval
Gate boundary. The gate sits between `ToolPolicy` and `ToolCall`: after
`ToolPolicyEngine` produces a `ToolPolicyOutcome`, the `ApprovalGate` inspects
the `approval_requirement` field to decide whether the tool may proceed
immediately or whether a human must approve first.

### Approval Module Types

| Type | Role |
|------|------|
| `ApprovalRequirement` | Enum with two variants: `None` (no approval needed) and `Manual { reason }` (human must approve before the tool runs) |
| `ApprovalGate` | Evaluates an `ApprovalRequirement` and returns `Some(ApprovalRequest)` when manual approval is required, or `None` when the tool may proceed |
| `ApprovalRequest` | Pending approval request surfaced to the operator; carries `tool`, `path`, `risk`, and `reason` fields |

### `EventKind::ApprovalRequest`

`EventKind::ApprovalRequest` is recorded when the `ApprovalGate` determines
that manual approval is required for a tool invocation. It sits between the
`ToolPolicy` event and the `ToolCall` event in the event sequence: a gate
evaluation returning `Some(ApprovalRequest)` would emit this event before the
tool may run.

### `approval_requirement` Field on `ToolPolicyOutcome`

`ToolPolicyOutcome` carries an `approval_requirement: ApprovalRequirement`
field that the `ApprovalGate` reads to determine whether to emit an
`ApprovalRequest`. This field is the bridge between the policy engine and the
gate.

### Production Behavior: Read-only Tools Run Without Approval

`ToolPolicyEngine::read_only()` — the engine wired into the production tool
harness — always sets `approval_requirement` to `ApprovalRequirement::None`.
`ApprovalGate::evaluate()` returns `None` for `ApprovalRequirement::None`, so
**no `ApprovalRequest` event is ever emitted on the production path**. Read-only
tools (`list_files`, `read_file`) proceed directly from `ToolPolicy` to
`ToolCall` without an approval step.

### Manual Path: Test-Only — No Tool-Execution Resume or UI

`ToolPolicyEngine::manual_for_test(reason)` is a `#[cfg(test)]`-gated
constructor that returns `ApprovalRequirement::Manual { reason }`. It exists
solely to let tests exercise the `ApprovalGate` → `ApprovalRequest` code path.
There is **no post-approval tool-execution resume and no interactive
confirmation UI** in this step: `/approval approve <seq>` and
`/approval reject <seq>` record an `ApprovalDecision` for a pending
`ApprovalRequest` but do **not** resume the underlying tool. The Manual path is
test-only and is never reachable through any production or user-visible code
path.

> **The `approval.rs` module is pure data** — it defines the types and does not
> import the event log, policy engine, or runner. The wiring lives elsewhere:
> `ToolPolicyEngine` sets `approval_requirement`, and `ToolEventRunner` already
> evaluates the `ApprovalGate` after `ToolPolicy` on the allow path (returning
> `None`, and so emitting no `ApprovalRequest`, for production read-only tools).
> What is deferred to a future task is resuming tool execution after an
> approval decision, an interactive approval UI, and any approval-requiring
> (write/shell) tool.

### `/approval status`

```
/approval status
```

Shows the **pending** approval queue. The queue is the `ApprovalQueue` projection —
a read-only view over the `EventLog` that collects `ApprovalRequest` events and
partitions them into `pending` (no valid matching `ApprovalDecision`) and `resolved`
(a valid `ApprovalDecision` event references the request seq). Only **pending**
requests are shown; requests resolved by a recorded `ApprovalDecision` are excluded.
A `ApprovalDecision` is **valid** for a request when it parses via
`ApprovalDecisionRecord`, references an existing `ApprovalRequest` seq, and its own
event seq is greater than that request seq; if several valid decisions reference the
same request, the one with the greatest decision seq wins.

After the pending queue, `/approval status` also prints the **approved resume plan**
count produced by `ApprovalQueue::resume_plans` — a read-only projection of resolved
approvals whose `decision` is `Approved` and whose tool name is supported (`read_file`
or `list_files`). For each such plan one summary line and the suggested `/tool`
command are printed. **No tool execution or resume is performed in this step.**

Example output when one request was approved and the pending queue is now empty:

```
Approval status:
- pending: none
- approved resume plans: 1
- seq=3 tool=read_file path="README.md" risk=read_only reason=operator_approved
- suggested: /tool read README.md
```

This command is **observe-only**:

- It does **not** approve or reject any pending request.
- It does **not** run the model or execute any tool.
- It does **not** resume tool execution — the suggested `/tool` command is a hint
  only; execution requires a manual `/tool read` or `/tool list` command.
- It appends only a `SlashCommand` event to the event log.
- `/approval approve <seq>` and `/approval reject <seq>` now exist and append
  an `ApprovalDecision` event, but they do **not** resume tool execution.

When there are no pending approvals and no approved resume plans, the output is:

```
Approval status:
- pending: none
- approved resume plans: 0
```

On the production read-only-tool path, `ApprovalRequest` events are never
emitted (see [Approval Gate Skeleton](#approval-gate-skeleton)), so
`/approval status` will always report `pending: none` and `approved resume plans: 0`
during normal use.

## Approval Resume Plan Projection

`ApprovalQueue::resume_plans()` is a read-only projection method on `ApprovalQueue`
(defined in `crates/kernel/src/approval_queue.rs`). It reads the `resolved` list,
keeps only entries whose `decision` is `Approved`, and for each one attempts to parse
the `request_detail` string via `ParsedApprovalRequest::parse_detail`. Entries that
fail to parse, or whose parsed tool name is not supported
(`ParsedApprovalRequest::to_tool_request` returns `None`), are silently dropped.
Each surviving entry becomes one `ApprovalResumePlan`.

### `ParsedApprovalRequest`

`ParsedApprovalRequest` (defined in `crates/kernel/src/approval.rs`) is a parsed
representation of an `ApprovalRequest` event detail string. It carries the same four
fields as `ApprovalRequest` (`tool`, `path`, `risk`, `reason`) and exposes two methods:

- `parse_detail(detail: &str) -> Option<Self>` — label-aware parser that handles
  Debug-quoted paths (paths with spaces, `=` characters, or escaped quotes).
- `to_tool_request() -> Option<ToolRequest>` — converts to a `ToolRequest` for
  recognised tool names (`read_file`, `list_files`); returns `None` for unsupported
  tool names.

### `ApprovalResumePlan`

`ApprovalResumePlan` (defined in `crates/kernel/src/approval.rs`) ties a
`ParsedApprovalRequest` to its approval/decision sequence numbers so that a later
step could use them to resume execution. It carries:

| Field | Type | Description |
|-------|------|-------------|
| `request_seq` | `EventSeq` | Seq of the original `ApprovalRequest` event |
| `decision_seq` | `EventSeq` | Seq of the `ApprovalDecision` event that resolved it |
| `request_detail` | `String` | Original event detail string (verbatim) |
| `request` | `ParsedApprovalRequest` | Parsed representation of the request |

`ApprovalResumePlan` also exposes:

- `to_tool_request() -> Option<ToolRequest>` — delegates to the inner
  `ParsedApprovalRequest::to_tool_request()`.
- `suggested_command() -> Option<String>` — returns the `/tool read <path>` or
  `/tool list <path>` command string the operator should run to replay the request;
  returns `None` for unsupported tool names.

> **No resume execution happens in this step.** `ApprovalQueue::resume_plans()` and
> `ApprovalResumePlan` are purely read-only projections. The suggested `/tool` command
> from `suggested_command()` is surfaced as a screen-log hint by `/approval status`;
> the tool is never invoked automatically and no `ToolCall`/`ToolResult`/`ToolError`
> event is emitted. Resume execution is handled by `/approval resume <seq>`.

## `/approval resume` Behavior

`/approval resume <seq>` resumes an approved `ApprovalResumePlan` by executing the
underlying read-only tool. The plan identified by `<seq>` must exist in the
`ApprovalQueue::resume_plans()` projection (i.e. it must correspond to an
`ApprovalRequest` resolved with `Approved` and a supported tool). For pending,
rejected, or unknown seqs the command does nothing.

**Consume-on-attempt (no-retry) policy:** The resume plan is consumed the moment the
`ApprovalResume` event is recorded — before the tool executes. Even if the tool
returns an error (`ToolError`), the plan is **not** retried and will no longer
appear in `/approval status`. A new `/approval approve <seq>` is required to
re-queue the same request.

**Event sequence on resume:**

```
SlashCommand → ApprovalResume → ToolPolicy → ToolCall → (ToolResult | ToolError)
```

**After a successful resume:** Run `/context attach-last-tool` to attach the tool
output to the next prompt. `pending_manual_tool_context` is **not** set
automatically — you must run the attach command manually.

## Manual Tool Commands

Two read-only slash commands expose the workspace tool harness directly from the
TUI input bar. Both commands are safe by design — they never write, delete, move,
or execute files.

### `/tool list [path]`

```
/tool list [path]
```

Lists the **immediate children** of a directory inside the workspace. `path` is
optional; when omitted the listing starts at the workspace root. Results are
returned in sorted order and are non-recursive (sub-directories appear as entries
but are not expanded).

**Example:**

```
/tool list src
```

### `/tool read <path>`

```
/tool read <path>
```

Reads a **UTF-8 text file** inside the workspace and displays its content.
`path` is required. Files larger than 64 KiB or containing invalid UTF-8 are
rejected with an error.

**Example:**

```
/tool read README.md
```

### Path Resolution and Safety

Both commands resolve `path` relative to the **workspace root**. The following
path forms are always rejected before any filesystem operation is attempted:

- **Absolute paths** — only workspace-relative paths are accepted.
- **Parent-directory traversals** (`../`) — any path that would escape the root
  via `..` components is rejected.
- **Symlink escapes** — resolved symlinks that point outside the workspace root
  are rejected.

Rejected paths return an error to the command bar; no filesystem operation is
performed.

### Events Emitted

Each `/tool` command appends three event kinds to the Event Log, in order:

| Event | When recorded |
|-------|--------------|
| `ToolCall` | Immediately before the tool executes; carries the tool name and input arguments |
| `ToolResult` | After a successful execution; carries a **summary only** — the entry count for `/tool list` or the byte count for `/tool read`, never the full file content |
| `ToolError` | After a failed execution (path rejected, file not found, size exceeded, etc.); carries the tool name and error detail |

`ToolResult` is deliberately summary-only: it records the number of directory
entries returned by `/tool list` or the number of bytes read by `/tool read`.
Full file content is never written to the event log, so event-log detail strings
remain bounded in size regardless of the file being read.

> **No write, shell, delete, or network tools exist in this stage.** Only the
> two read-only tools (`/tool list` and `/tool read`) are available.

## Manual Tool Context

Manual tool context lets you attach the output of a read-only tool call to the
next prompt you send to the model. The attach is **one-shot**: the context is
automatically cleared after the run completes, so it never leaks into subsequent
turns. The attached output is **bounded to 4096 bytes** (including any truncation
marker); larger tool outputs are truncated before being inserted into the prompt.

### One-shot attach flow

1. Run `/tool read <path>` or `/tool list [path]` to execute a read-only tool.
2. Run `/context attach-last-tool` to stage that output as pending context. If a
   recent successful `/tool read` or `/tool list` result exists, a
   `ToolContextAttach` event is appended (with a summary-only detail). If no tool
   output is available, a notice is shown and no `ToolContextAttach` event is
   emitted.
3. Submit your next plain-text message. The pending tool output is injected into
   the `Workspace Context:` section of the compiled prompt for that turn.
4. After the run completes, the pending context is automatically cleared — it is
   not carried forward into future turns (this auto-clear emits no event).

To discard staged context before it is used, run `/context clear`. A
`ToolContextClear` event is appended and the pending context is removed.

### Source Label and Summary Formats

Every `ManualToolContext` carries a canonical **source label** that identifies
the tool, path, risk level, and truncation state without embedding any file
content:

```
tool=<read_file|list_files> path="<path>" risk=read_only truncated=<bool>
```

For example, reading `README.md` with no truncation produces:

```
tool=read_file path="README.md" risk=read_only truncated=false
```

`risk=read_only` is always present — every `ManualToolContext` is produced by a
read-only tool, so no other risk level is possible in this POC. `truncated=`
reflects whether the stored `content` was trimmed to fit the 4096-byte cap.

#### `/context status` and `ToolContextAttach` summary format

`/context status` and the `ToolContextAttach` event `detail` field use an
**extended summary** that appends a `bytes=<n>` byte-count field:

```
tool=read_file path="README.md" risk=read_only bytes=<n> truncated=<bool>
```

This summary is **summary-only** — it never includes the raw file content.
Full file content appears **only** in the PromptCompile `Workspace Context:` section
(see below), never in `/context status` output or the `ToolContextAttach` event
detail.

#### PromptCompile `Workspace Context:` section

When pending manual tool context is present, the prompt compiler renders a
structured block inside the `Workspace Context:` section:

```
Workspace Context:
Manual Tool Context:
Source:
  tool=read_file path="README.md" risk=read_only truncated=false
Content:
<bounded file content, at most 4096 bytes>
```

When no manual tool context has been staged, the `Workspace Context:` section contains
the fallback literal exactly as written:

```
Workspace Context:
No external tool context is available.
```

This fallback is also the text shown for any turn where context was not attached
(including after a one-shot context has been auto-cleared following its turn).

### Sensitive-data warning

> **Warning:** `/context attach-last-tool` includes the bounded output of the
> user-chosen read-only tool call directly in the prompt sent to the model. For
> `/tool read` this may include (possibly truncated) file content; for
> `/tool list` this includes directory listing output. Do **not** read and attach
> sensitive files or directories — such as private keys, credentials, `.env`
> files, or any path containing secrets — because the attached output will be
> forwarded to the model layer. Review the tool output in the Inspector panel
> before attaching.

## Model-visible Read-only Tool Schema Skeleton

`crates/kernel/src/tool/schema.rs` defines the static schema types that describe
available tools in a form the model can read — plain Rust structs with a plain-text
renderer, **not** JSON Schema, OpenAI function-calling JSON, or MCP tool definitions.

### Kernel Types

| Type | Role |
|------|------|
| `ToolCatalog` | Static catalog holding all `ToolSpec` entries; constructed via `ToolCatalog::readonly()` |
| `ToolSpec` | Describes a single tool: `name`, `description`, `risk` (`ToolRisk`), and `inputs` (`Vec<ToolInputSpec>`) |
| `ToolInputSpec` | Describes a single input parameter: `name`, `description`, and `required` flag |

### Exposed Read-only Specs

`ToolCatalog::readonly()` populates the catalog with exactly two specs:

| Tool spec | Slash command | `path` input |
|-----------|---------------|--------------|
| `list_files` | `/tool list [path]` | optional — defaults to `"."` |
| `read_file` | `/tool read <path>` | required |

Both specs carry `risk: ToolRisk::ReadOnly`. No write, shell, delete, or network
tool spec is registered.

### `ToolCatalog::render_prompt_section()` — Experimental Harness Only

`ToolCatalog::render_prompt_section()` produces a plain-text block that begins with
an `Available Tools:` header, followed by a guidance paragraph and one entry per
tool (name, slash command, risk, description, and inputs).

> **This generator is experimental-harness-only.** The default Claude-baseline
> prompt does **not** call `render_prompt_section()` and does **not** include an
> `Available Tools` section. The method exists as a structural seam for future
> experimentation and is not wired into the `compile_prompt_with_context` path
> used by the standard Run/Turn flow.

### No Automatic Tool Calling

All tool execution is manual:

1. Run `/tool list [path]` or `/tool read <path>` to execute a read-only tool.
2. Run `/context attach-last-tool` to stage the tool output as pending prompt context.
3. Submit your next plain-text message — the tool output is injected into the
   `Workspace Context:` section of the compiled prompt for that turn only.

Automatic or model-driven tool calling is explicitly out of scope for this POC.

## Model Tool Request Detection (Detect-Only)

When the assistant response contains a `CARAVAN_TOOL_REQUEST` block, Caravan
detects it and records a `ModelToolRequest` event in the Event Log. This is a
**detect-only** mechanism — Caravan does **not** execute the requested tool
automatically.

### `CARAVAN_TOOL_REQUEST` Block Format

The model may embed a tool request in its response using the following format.
The delimiter lines must appear exactly as bare text — no angle brackets or other
decoration — and keys use `=` as the separator.

**`read_file` example:**

```
CARAVAN_TOOL_REQUEST
tool=read_file
path=README.md
END_CARAVAN_TOOL_REQUEST
```

**`list_files` example:**

```
CARAVAN_TOOL_REQUEST
tool=list_files
path=.
END_CARAVAN_TOOL_REQUEST
```

When a block matching these markers is present in the assistant response, Caravan
records a `ModelToolRequest` event whose detail carries the detected block
contents.

### What Detection Does NOT Do

Detection does **not**:

- Execute the named tool.
- Produce a `ToolCall`, `ToolResult`, or `ToolError` event — those events are
  emitted only when the user explicitly runs a `/tool` command.

The `ModelToolRequest` event is a trace only. No tool runs, no output is
produced, and the prompt for the next turn is unaffected unless the user
completes the steps below.

### What You Must Do Manually

1. Observe the `ModelToolRequest` event in the Event Log (and its detail in the
   Inspector) to see which tool the model requested.
2. Run the matching slash command yourself — for example `/tool read <path>` or
   `/tool list [path]`.
3. Run `/context attach-last-tool` to stage the tool output as pending context.
4. Submit your next plain-text message — the tool output is injected into the
   `Workspace Context:` section of the compiled prompt for that turn only.

Without step 2, the `ModelToolRequest` event is recorded but the tool is never
run and no output enters the prompt. Detection is strictly observe-and-act —
there is no automatic execution.

### Screen-Log Guidance

When Caravan detects a `CARAVAN_TOOL_REQUEST` block it shows a guidance message
in the screen log. The guidance consists of:

- **Detected request detail** — the tool name and path extracted from the block.
- **Explicit notice** that Caravan did **not** execute the request automatically.
- **Suggested `/tool` command** — `/tool read <path>` for a `read_file` request,
  or `/tool list <path>` for a `list_files` request.
- **Next step** — run `/context attach-last-tool` after the tool command
  completes to stage the output as pending context.

This guidance is a **screen-log UI hint only**. It is **NOT** recorded as an
Event Log event and does not appear in the Inspector. The tool still runs only
when the user runs the suggested `/tool` command manually.

> **Limitation:** Paths with spaces are not supported yet. The suggested command
> uses the raw path verbatim; use a path without spaces.

### Pending Model Tool Request UX

When Caravan detects a `CARAVAN_TOOL_REQUEST` block, it stores the detected
request as a **pending suggested action** in memory — replacing any previously
stored pending request. The pending state is surfaced through two commands and
the `| Request:` header segment described above.

#### `/request status`

Shows the currently pending model tool request. The output includes:

- The suggested `/tool` command to run (e.g. `/tool read <path>` or
  `/tool list <path>`).
- A reminder to run `/context attach-last-tool` as the next step after the tool
  command completes.

`/request status` is **read-only** — it does not run the model, does not execute
any tool, and does not modify the pending state.

#### `/request run`

Explicitly executes the pending model tool request as a read-only tool. This is
the **only** way a model-requested tool is ever run — the model response alone
never causes a tool to execute automatically.

**Success path** — when a pending request exists and the tool executes without
error:

1. A `SlashCommand` event is recorded for the `/request run` command.
2. A `ToolCall` event is appended immediately before the tool runs, carrying the
   tool name and input arguments.
3. A `ToolResult` event is appended after a successful execution, carrying a
   summary of the output (never the full file content).
4. A preview of the tool output is shown in the screen log.
5. The manual tool output candidate is updated so that a subsequent
   `/context attach-last-tool` picks up this result.
6. The pending model tool request is cleared (header returns to
   `| Request: none`).
7. A prompt to run `/context attach-last-tool` is shown in the screen log.

**Failure path** — when the tool executes but returns an error:

1. A `SlashCommand` event is recorded.
2. A `ToolCall` event is appended.
3. A `ToolError` event is appended, carrying the tool name and error detail.
4. The pending model tool request is **kept** — the request remains pending so
   you can correct the problem and retry.

**No-pending path** — when no request is currently pending:

- The message `No pending model tool request.` is shown in the screen log.
- No tool is run, no model is called, and no event is appended beyond the
  `SlashCommand` event for `/request run`.

#### `/request clear`

Removes the pending model tool request. After `/request clear`, the header shows
`| Request: none` and `/request status` reports that no request is pending.

`/request clear` is **read-only** — it does not run the model and does not execute
any tool.

#### Behavior Contract

The following invariants apply to the pending model tool request state:

- A detected `ModelToolRequest` replaces any previously pending request — there
  is at most one pending request at a time.
- The pending request is **not** executed automatically; Caravan never runs a
  tool on behalf of the model.
- A plain model response (one that contains no `CARAVAN_TOOL_REQUEST` block)
  does **not** clear the pending request — it remains pending until explicitly
  cleared via `/request clear`.
- A successful `/tool` command does **not** auto-clear the pending request — you
  must run `/request clear` explicitly when you are done with it.
- The pending state is **in-memory only** — it is not persisted to
  `.caravan/events.jsonl` and is not restored across restarts.

## Manual Verification

The following checks must be confirmed interactively before the POC is considered done:

- [ ] `cargo run` opens the TUI showing the Header (`Caravan | TUI Shell | Status: Ready`),
      the Nav/Main/Inspector columns, the Log panel, and the Command Bar — without panicking.
- [ ] Submitting plain text (no leading `/`) shows `User: <text>` and
      `Assistant: Mock response for: <text>` lines in the Main panel, and appends
      the full Run/Turn event sequence (`UserMessage`, `RunCreate`, `RunStart`,
      `TurnStart`, `PromptCompile`, `ModelRoute`, `ModelOutputChunk` × N, `AssistantMessage`, `RunComplete`) to the Event Log.
- [ ] `/help` appends the command list to the Log only; Main panel is unchanged.
- [ ] `/clear` empties the Log panel; the Event Log retains all previous entries.
- [ ] An unknown command (e.g. `/foo`) appends an `Unknown command:` line to the Log.
- [ ] `/exit` exits the app cleanly and the terminal is fully restored (no raw-mode residue,
      cursor and normal screen returned).
- [ ] Pressing Down selects the first event; Inspector shows its seq, kind, and message.
- [ ] Pressing Up and Down navigates between events; the selected row is highlighted in the
      Event Log panel.
