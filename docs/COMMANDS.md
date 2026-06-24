# Caravan Command Reference

Commands are grouped by layer. The Claude-like core commands form the primary UX.
The experimental harness commands are a structural seam for future agentic tooling.

---

## 1. Claude-like Core Commands

These commands mirror the session-control commands in Claude Code and form the
primary interface for all Caravan users.

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

---

## 2. Experimental Caravan Harness Commands

These commands are part of the **experimental harness layer** — a structural seam for
future agentic tooling. They are not the primary UX and may change substantially.

### Tool commands

| Command | Description |
|---------|-------------|
| `/tool list [path]` | List files under the workspace root (or a sub-path); read-only |
| `/tool read <path>` | Read a UTF-8 text file under the workspace root; read-only |
| `/tool plan-write <path>` | Record a `workspace_write` mutation intent and route it through the approval gate; **performs no real write** and produces no `ToolCall`/`ToolResult`/`ToolError` |
| `/tool preview-write <path>` | Dry-run diff preview of what a write would produce; **performs no real write** |
| `/tool propose-write <path>` | Preview-backed approval request; **performs no real write** |

> **No actual file write is performed by any `/tool *-write` command.** File mutation
> is still not implemented; these commands exist as a sandbox skeleton only.

### Context commands

| Command | Description |
|---------|-------------|
| `/context attach-last-tool` | Attach the latest read-only tool output to the next prompt (one-shot) |
| `/context clear` | Discard any pending manual tool context |
| `/context status` | Print a read-only status report of pending manual tool context and the last tool-output candidate |

### Request commands

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

## 3. Reserved — Claude Code Commands Not Implemented Yet

The following slash commands are intentionally reserved to match the Claude Code
command namespace. None of them are implemented yet; entering any of these in the
current build produces an `UnknownSlashCommand` event.

| Command | Status |
|---------|--------|
| `/model` | Reserved — not implemented yet |
| `/plan` | Reserved — not implemented yet |
| `/diff` | Reserved — not implemented yet |
| `/resume` | Reserved — not implemented yet |
| `/status` | Reserved — not implemented yet |
| `/usage` | Reserved — not implemented yet |
| `/init` | Reserved — not implemented yet |
| `/memory` | Reserved — not implemented yet |

---

## 4. Explicitly Unsupported Inputs

The following inputs have no planned implementation path at this stage and map to
`UnknownSlashCommand` in the event log. They are listed here so that the command
set boundary is explicit.

| Input | Reason unsupported |
|-------|--------------------|
| `/ask` | Not a Caravan command; maps to `UnknownSlashCommand` |
| `/tool write` | Not a valid sub-command; write execution is not implemented — use `/tool plan-write`, `/tool preview-write`, or `/tool propose-write` for the skeleton harness |
| `/approval run` | Not a valid sub-command; use `/approval resume <seq>` to execute an approved plan |
| Any other unrecognised `/command` | Maps to `UnknownSlashCommand`; an `Unknown command: /…` notice is shown in the screen log |
