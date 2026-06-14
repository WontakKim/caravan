# Caravan

A minimal Rust TUI shell skeleton. Agents, models, and tools are out of scope for this POC.

## Workspace Structure

The repository is a Cargo workspace with a root virtual manifest (`Cargo.toml`)
and three crates under `crates/`:

| Crate | Path | Responsibility |
|-------|------|---------------|
| `kernel` | `crates/kernel` | TUI-free logic: commands, events, storage, prompt compiler, runner, model layer |
| `tui` | `crates/tui` | App state, input handling, and rendering (depends on `kernel`) |
| `cli` | `crates/cli` | Binary entrypoint; produces the `caravan` binary (depends on `kernel` and `tui`) |

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
   System/User/Context/Output template; the event `detail` holds the compiled
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

`runner::run_mock_turn(event_log, message, gateway)` owns the full Run/Turn lifecycle.
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
to the model. The prompt compiler produces a fixed five-section template:

```
System:
You are Caravan's local assistant.

Conversation:
No prior conversation context.

Current User:
<message>

Context:
No external tool context is available in this POC.

Output:
Respond to the current user message.
```

`compile_prompt(message)` renders the empty-history case shown above. It
delegates to `compile_prompt_with_context(message, history)`, which fills the
`Conversation:` section from recent transcript messages — see
[Prompt Context Window](#prompt-context-window). The result is stored in the
`PromptCompile` event `detail` field as the compiled prompt preview. When you
select a `PromptCompile` event in the **Inspector** panel, the panel displays
this full System / Conversation / Current User / Context / Output preview,
letting you inspect exactly what was compiled for that turn.

## ModelAdapter Boundary

`runner::run_mock_turn` owns the Run/Turn lifecycle and event append — it
appends `RunCreate → RunStart → TurnStart → PromptCompile → ModelRoute →
ModelOutputChunk* → AssistantMessage → RunComplete` — but it no longer contains inline response or token generation
logic. Those responsibilities are delegated to a `ModelAdapter`.

The `ModelAdapter` trait (defined in `crates/kernel/src/model.rs`) exposes a single method:

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
`runner.rs`/`crates/kernel/src/model.rs`/gateway wiring change rather than an App-layer change.

## ModelGateway Boundary

`ModelRequest is now defined in` `crates/kernel/src/model.rs` as the shared core adapter request type used by `ModelAdapter`, `ModelAdapterRegistry`, `ModelGateway`, and the runner — no longer a gateway-local type.

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
integrations require changes only inside `ModelGateway` and `crates/kernel/src/model.rs`,
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

`OpenAICompatibleAdapter` lives in `crates/kernel/src/model_openai_compatible.rs` and
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

`crates/kernel/src/model_openai_types.rs` defines the five serde-serializable structs that
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

`OpenAICompatibleConfig` in `crates/kernel/src/model_openai_config.rs` is the configuration
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

`OpenAIRequestBuilder::build` in `crates/kernel/src/model_openai_request.rs` combines an `OpenAICompatibleConfig`,
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

`crates/kernel/src/model_openai_http.rs` defines the synchronous HTTP client boundary for
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

`ModelUsage` is a plain value type defined in `crates/kernel/src/model.rs`:

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
- **Helper:** `compile_prompt_with_context(current_user_message, history)` owns
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

`crates/kernel/src/tools.rs` contains a minimal tool harness for read-only
workspace inspection. The harness is **kernel-only** and is intentionally
**not wired** to the model, runner, TUI slash commands, or the EventLog in
this POC — it records no events and produces no observable side effects during
a normal run.

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
| `ToolCall` | Immediately before the tool executes; carries the tool name and input arguments |
| `ToolResult` | After a successful execution; carries a **summary only** — never the full file content |
| `ToolError` | After a failed execution; carries the tool name and error detail |

`ToolResult` stores only a short human-readable summary of the outcome (e.g. a
file-listing count or a byte-length notice), never the raw file content returned
by `read_file` or `list_files`. This keeps event-log detail strings bounded in
size regardless of the files being read.

> **This boundary is intentionally not wired to the model, runner, or TUI slash
> commands in this POC.** `ToolEventRunner` exists solely as a kernel-level
> tracing boundary. No slash command triggers tool execution, no model output
> causes a tool call, and no TUI panel renders tool results. The `ToolCall`,
> `ToolResult`, and `ToolError` events are appended to the `EventLog` only when
> `ToolEventRunner` is exercised directly (e.g. in unit tests).

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
