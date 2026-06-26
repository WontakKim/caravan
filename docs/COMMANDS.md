# Caravan Command Reference

Commands are organized by their role in the UX hierarchy: the default surface
is what all users interact with first; the hidden harness commands are implemented
infrastructure that is not the center of the default UX; reserved commands are
placeholders matching the Claude Code namespace.

---

## 1. Default Command Surface

These are the commands that every Caravan user is expected to use in normal
operation. They form the primary UX surface.

### Claude-like Core Commands

These commands mirror the session-control commands in Claude Code.

| Command | Description |
|---------|-------------|
| `/help` | Show the list of available commands |
| `/clear` | Clear the screen log (Main panel); the append-only Event Log is unaffected |
| `/exit` | Exit the application cleanly |
| `/quit` | Exit the application (alias for `/exit`) |
| `/reset` | Reset the session: clears the conversation transcript and starts a fresh session |
| `/new` | Start a new conversation, clearing the current conversation context |
| `/permissions` | Display the current tool permission and policy settings |
| `/allowed-tools` | List the tools currently permitted in this session |
| *(plain text)* | Submit a task to the assistant; runs the mock Run/Turn flow and appends `User:` / `Assistant:` output to the Main panel |

**Project memory:** If a `CLAUDE.md` file is present at the workspace root, its content
is loaded at session start by the `project_memory` module and injected into the main
prompt. There is no automatic secret detection — do not place credentials in `CLAUDE.md`.

### Basic Workspace Tools

Read-only tool commands for inspecting the workspace. These are part of the
default UX surface: they observe the filesystem without mutating it.

| Command | Description |
|---------|-------------|
| `/tool list [path]` | List files under the workspace root (or a sub-path); read-only. On success, automatically attaches the bounded listing as the next message's Workspace Context (one-shot). |
| `/tool read <path>` | Read a UTF-8 text file under the workspace root; read-only. On success, automatically attaches the bounded file content as the next message's Workspace Context (one-shot). |
| `/tool search <query>` | Search for text across workspace files; read-only. On success, automatically attaches the bounded results as the next message's Workspace Context (one-shot). |

> **Manual path vs. native path:** `/tool list` and `/tool read` are the *manual*
> path — they execute a tool on direct user request and stage the result as
> Workspace Context for the next prompt. When a real model adapter is active, the
> model may also call `list_files` / `read_file` *natively* via the API `tools`
> field (at most 1 tool call per turn, bounded to 2 model calls total); that result
> is fed directly back to the model and does **not** populate the manual Workspace
> Context. `/context attach-last-tool` applies only to the manual path.
> See the README "Native Read-only Tool Calling" section for details.

> **Sensitive-file warning:** A successful `/tool read` or `/tool list` automatically
> includes the bounded output in the next prompt sent to the model. Do **not** use
> these commands on sensitive files or directories — such as private keys,
> credentials, `.env` files, or any path containing secrets — because the content
> will be forwarded to the model layer automatically. `/context attach-last-tool`
> also includes the output when used explicitly; the same caution applies.

---

## 2. Hidden / Internal Experimental Harness Commands

These commands are **implemented and still parse**, but they are **not the center
of the default UX**. They exist as internal infrastructure — a structural seam for
future agentic tooling. They may change substantially and are not surfaced in
`/help` by default.

### Tool write-staging commands

> **No actual file write is performed by any `/tool *-write` command.** File
> mutation is still not implemented; these commands exist as a sandbox skeleton
> only.

| Command | Description |
|---------|-------------|
| `/tool plan-write <path>` | Record a `workspace_write` mutation intent and route it through the approval gate; **performs no real write** and produces no `ToolCall`/`ToolResult`/`ToolError` |
| `/tool preview-write <path>` | Dry-run diff preview of what a write would produce; **performs no real write** |
| `/tool propose-write <path>` | Preview-backed approval request; **performs no real write** |

### Context commands

> **Hidden / compatibility / advanced context controls.** These commands expose the workspace
> context layer directly. For normal use, `/tool read` and `/tool list` stage workspace context
> automatically; the `/context` commands are provided for advanced workflows and compatibility.

| Command | Description |
|---------|-------------|
| `/context attach-last-tool` | Attach the latest read-only tool output to the next prompt as workspace context (one-shot) |
| `/context clear` | Discard any pending workspace context |
| `/context status` | Print a read-only status report of pending workspace context and the last tool-output candidate |

### Request commands

> **Experimental — not connected to the default runtime.** The default runtime
> does **not** auto-detect model tool requests. `/request` commands exist solely
> as internal/experimental compatibility infrastructure — a structural seam for
> future agentic tooling. They are **not connected to the default runtime** and
> must not be treated as part of the basic command surface. For basic tool
> invocation, use `/tool read` and `/tool list` (see
> [Basic Workspace Tools](#basic-workspace-tools) above).

| Command | Description |
|---------|-------------|
| `/request status` | Show the pending model tool request; does not run the model or any tool |
| `/request run` | Execute the pending model tool request as a read-only tool |
| `/request clear` | Clear the pending model tool request |

### Approval commands

| Command | Description |
|---------|-------------|
| `/approval status` | Show the pending approval queue and approved resume plan summary; observe-only |
| `/approval approve <seq>` | Record an `ApprovalDecision` (approved) for the pending `ApprovalRequest` at `<seq>`; does **not** resume tool execution |
| `/approval reject <seq>` | Record an `ApprovalDecision` (rejected) for the pending `ApprovalRequest` at `<seq>`; does **not** resume tool execution |
| `/approval resume <seq>` | Resume an approved `ApprovalResumePlan` by executing the underlying read-only tool; the plan is consumed on attempt |

---

## 3. Unsupported / Reserved Claude Code Commands

The following slash commands are intentionally reserved to match the Claude Code
command namespace or are explicitly unsupported. None of them are implemented
yet; entering any of these in the current build produces an `UnknownSlashCommand`
event.

| Command | Status |
|---------|--------|
| `/model` | Reserved — not implemented yet |
| `/plan` | Reserved — not implemented yet |
| `/diff` | Reserved — not implemented yet |
| `/resume` | Reserved — not implemented yet |
| `/status` | Reserved — not implemented yet |
| `/usage` | Reserved — not implemented yet |
| `/agents` | Reserved — not implemented yet |
| `/mcp` | Reserved — not implemented yet |
| `/memory` | Reserved — not implemented yet |
| `/ask` | Unsupported — not a Caravan command; maps to `UnknownSlashCommand` |
| `/tool write` | Unsupported — not a valid sub-command; write execution is not implemented — use `/tool plan-write`, `/tool preview-write`, or `/tool propose-write` for the skeleton harness |
| `/approval run` | Unsupported — not a valid sub-command; use `/approval resume <seq>` to execute an approved plan |
| Any other unrecognised `/command` | Maps to `UnknownSlashCommand`; an `Unknown command: /…` notice is shown in the screen log |

---

## 4. Rationale

The ordering of sections reflects the intended UX progression:

1. **Basic Claude-like interaction first.** The core session-control commands
   (`/help`, `/clear`, `/reset`, `/new`, `/exit`, `/quit`, `/permissions`,
   `/allowed-tools`) mirror the Claude Code UX that users already know. They are
   the default entry point and require no knowledge of the harness layer.

2. **Basic tool invocation before agent/tool automation.** Read-only inspection
   commands (`/tool list`, `/tool read`) are included in the default surface
   because they are safe, predictable, and useful on their own. Agent-driven
   tool automation (context attachment, request routing, approval gating) belongs
   to the harness layer and is deliberately hidden from the primary UX.

3. **Harness later.** The experimental harness commands are implemented and
   functional, but they are internal infrastructure. Surfacing them as primary
   commands would imply a level of stability and UX commitment that does not yet
   exist. They remain hidden until the agentic workflow is ready to be the center
   of the UX.

4. **No automatic mutation.** None of the default-surface commands mutate the
   filesystem. The harness write-staging commands (`/tool plan-write`,
   `/tool preview-write`, `/tool propose-write`) explicitly perform no real write.
   File mutation is a deliberate future step that requires approval gating to be
   fully implemented first.
