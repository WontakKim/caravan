# Caravan

A minimal Rust TUI shell skeleton. Agents, models, and tools are out of scope for this POC.

## Running

```sh
cargo run
```

## Available Commands

| Command          | Description                                              |
|------------------|----------------------------------------------------------|
| `/help`          | Show the list of available commands                      |
| `/clear`         | Clear the log panel                                      |
| `/exit`          | Exit the application                                     |

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
Log. Each navigation step appends an `InspectorSelection` event to the
log recording the newly selected `seq`.

- **Down** — move to the next (newer) event; no-op at the bottom boundary.
- **Up** — move to the previous (older) event; no-op at the top boundary.

### EventKind Values

| EventKind                    | When it is recorded                                      |
|------------------------------|----------------------------------------------------------|
| `AppStart`                   | Once, when the application initialises                   |
| `SlashCommand`               | Recorded for slash commands only (not plain text)        |
| `HelpRequest`                | When `/help` is processed                                |
| `UserMessage`                | When plain (non-command) text is submitted               |
| `LogClear`                   | When `/clear` is processed                               |
| `InspectorSelection`         | Each time the Up/Down selection changes                  |
| `ExitRequest`                | When `/exit` is processed or Ctrl+C is pressed           |
| `UnknownSlashCommand`        | When an unrecognised `/command` is entered               |
| `RunCreate`                  | When a new Run is initialised for a submitted user message|
| `RunStart`                   | When the Run begins executing (before the first Turn)    |
| `TurnStart`                  | When a Turn begins within a Run                          |
| `PromptCompile`              | When the prompt compiler assembles the structured prompt; `detail` holds the compiled prompt preview |
| `ModelRoute`                 | After `PromptCompile`, before the first `ModelToken`; carries mock provider/model/adapter route metadata selected by `ModelGateway` |
| `ModelToken`                 | Each token emitted during the mock model reply           |
| `RunComplete`                | When the Run finishes successfully                       |
| `RunFail`                    | Emitted when a Run fails on the model error path (after a `ModelError` event); no `ModelToken` or `RunComplete` is appended |
| `ModelError`                 | Emitted when the model layer returns an error; carries `kind=... message="..."` detail |

## Mock Run/Turn Flow

Submitting plain text (any input not starting with `/`) is a **deterministic
mock** — it does not call a real LLM. The reply is always
`Mock response for: <text>`, split into one `ModelToken` event per word.

### Event sequence

When `hello world` is entered, the following events are appended in order:

1. `UserMessage` — the submitted text is recorded (no `SlashCommand`).
2. `RunCreate` — a new Run is created; `run_id` is stored in the event `detail`.
3. `RunStart` — the Run transitions to the running state.
4. `TurnStart` — the first (and only) Turn begins; `turn_id` is in `detail`.
5. `PromptCompile` — the prompt compiler processes the input into the
   System/User/Context/Output template; the event `detail` holds the compiled
   prompt preview.
6. `ModelRoute` — `ModelGateway` selects the provider/model/adapter route; the
   event `detail` carries the route metadata (mock provider, model, and adapter).
7. `ModelToken` × N — one event per word in `Mock response for: <text>`.
8. `RunComplete` — the Run finishes successfully.

### Main panel output

After submitting plain text, the Main panel shows:

```
User: <text>
Assistant: Mock response for: <text>
```

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
Run/Turn event assembly to `src/runner.rs`.

`runner::run_mock_turn(event_log, message, gateway)` owns the full Run/Turn lifecycle.
It appends the sequence `RunCreate → RunStart → TurnStart → PromptCompile →
ModelRoute → ModelToken* → RunComplete` to the event log (but **not**
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
to the mock model. The prompt compiler produces a fixed four-section template:

```
System:
You are Caravan's local mock assistant.

User:
<message>

Context:
No external context is available in this POC.

Output:
Respond with a deterministic mock response.
```

The result is stored in the `PromptCompile` event `detail` field as the compiled
prompt preview. When you select a `PromptCompile` event in the **Inspector**
panel, the panel displays this full System / User / Context / Output preview,
letting you inspect exactly what was compiled for that turn.

## ModelAdapter Boundary

`runner::run_mock_turn` owns the Run/Turn lifecycle and event append — it
appends `RunCreate → RunStart → TurnStart → PromptCompile → ModelRoute →
ModelToken* → RunComplete` — but it no longer contains inline response or token generation
logic. Those responsibilities are delegated to a `ModelAdapter`.

The `ModelAdapter` trait (defined in `src/model.rs`) exposes a single method:

```rust
fn complete(&self, request: &ModelRequest) -> ModelResult<ModelOutput>;
```

`ModelOutput` carries two fields:

```rust
pub struct ModelOutput {
    pub response: String,
    pub tokens: Vec<String>,
}
```

`runner::run_mock_turn` no longer calls `MockModelAdapter` directly. Instead it
delegates to `ModelGateway`, which calls `MockModelAdapter.complete` internally
(see [ModelGateway Boundary](#modelgateway-boundary)). The runner iterates
`ModelOutput.tokens` to append one `ModelToken` event per token, and stores
`ModelOutput.response` for the `Assistant:` line in the Main panel.

`MockModelAdapter` is the concrete implementation used in the POC. It produces
a deterministic `"Mock response for: <message>"` response and splits it via
`split_whitespace()` to derive the token list. The mock receives a `&ModelRequest`
and simply leaves `request.prompt` unread.

This is a **structural boundary only** — user-visible behavior and the event
sequence are unchanged. The boundary gives Caravan a clear seam for a real model
adapter while keeping the App layer insulated; because `ModelGateway` today
wraps the concrete `MockModelAdapter`, introducing a real adapter is a localized
`runner.rs`/`src/model.rs`/gateway wiring change rather than an App-layer change.

## ModelGateway Boundary

`ModelRequest is now defined in` `src/model.rs` as the shared core adapter request type used by `ModelAdapter`, `ModelAdapterRegistry`, `ModelGateway`, and the runner — no longer a gateway-local type.

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
integrations require changes only inside `ModelGateway` and `src/model.rs`,
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
`runner::run_mock_turn(event_log, message, gateway)` on every call.

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
plain strings. Both are defined in `src/model_types.rs`:

- **`ModelProvider`** — variants: `Mock`. Exposes `as_str()` returning `"mock"`.
- **`ModelAdapterKind`** — variants: `MockModelAdapter`. Exposes `as_str()` returning
  `"MockModelAdapter"`.

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
3. Does **not** append any `ModelToken` or `RunComplete` events.

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

`OpenAICompatibleAdapter` lives in `src/model_openai_compatible.rs` and
implements the `ModelAdapter` trait. Its `complete()` method makes **no real
network call** and always returns `Err(ModelError::AdapterFailure)`. This is a
skeleton only — there is no real API or network integration of any kind.

Two new typed variants have been added to the enums in `src/model_types.rs`:

| Enum | Variant | `as_str()` value |
|------|---------|-----------------|
| `ModelProvider` | `OpenAICompatible` | `"openai-compatible"` |
| `ModelAdapterKind` | `OpenAICompatibleAdapter` | `"OpenAICompatibleAdapter"` |

`ModelAdapterRegistry` owns the `OpenAICompatibleAdapter` instance as a normal
field (alongside `MockModelAdapter`). Adapter selection is driven by matching on
`ModelAdapterKind` inside the registry — `ModelGateway` delegates to the
registry as before and is unaware of the concrete adapter type.

`OpenAICompatible` profiles (i.e. `ModelProfile` values whose `adapter` field is
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

## Manual Verification

The following checks must be confirmed interactively before the POC is considered done:

- [ ] `cargo run` opens the TUI showing the Header (`Caravan | TUI Shell | Status: Ready`),
      the Nav/Main/Inspector columns, the Log panel, and the Command Bar — without panicking.
- [ ] Submitting plain text (no leading `/`) shows `User: <text>` and
      `Assistant: Mock response for: <text>` lines in the Main panel, and appends
      the full Run/Turn event sequence (`UserMessage`, `RunCreate`, `RunStart`,
      `TurnStart`, `PromptCompile`, `ModelRoute`, `ModelToken` × N, `RunComplete`) to the Event Log.
- [ ] `/help` appends the command list to the Log only; Main panel is unchanged.
- [ ] `/clear` empties the Log panel; the Event Log retains all previous entries.
- [ ] An unknown command (e.g. `/foo`) appends an `Unknown command:` line to the Log.
- [ ] `/exit` exits the app cleanly and the terminal is fully restored (no raw-mode residue,
      cursor and normal screen returned).
- [ ] Pressing Down selects the first event; Inspector shows its seq, kind, and message.
- [ ] Pressing Up and Down navigates between events; the selected row is highlighted in the
      Event Log panel.
