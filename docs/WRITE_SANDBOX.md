# Write Tool and Sandbox Safety Design

## Purpose

This document defines the safety contract Caravan must honor **before** implementing
any real workspace mutation tool. Only read-only tools (`list_files`, `read_file`)
execute today. The `/tool plan-write` command is an approval-queue and
mutation-intent skeleton: it records intent and queues an approval request, but does
not perform any file I/O. This document is a **design baseline**, not an
implementation — nothing described in the "future" sections exists in code today.

## Current State

**Read-only tools:** `list_files` and `read_file` are the only tools that execute
today. `ToolCatalog::readonly()` exposes only these two tools.

**`/tool plan-write <path>`:** Submitting this command records exactly three new
events in the EventLog:
1. `SlashCommand` — logged generically for every slash command.
2. `ToolPolicy` — records the tool-specific policy evaluation result.
3. `ApprovalRequest` — queues the mutation intent for user review.

The command writes **no file** and records **no** `ToolCall`, `ToolResult`, or
`ToolError`. The `PlanWrite` dispatch arm exists in `ToolRegistry::execute()` and
short-circuits by returning `Err(ToolError::ApprovalRequired)` — the handler exists;
only the write I/O is absent. In short, `/tool plan-write` writes no file and
executes no tool — it does not write a file.

**`/approval approve|reject`:** After the generic `SlashCommand` event, these
commands append exactly one approval-specific event — an `ApprovalDecision`. No
execution or file I/O occurs.

**`/approval resume`:** Can execute only an approved **read-only** resume plan.
A `write_file` approval is **not** converted into an `ApprovalResumePlan`:
`to_tool_request()` in `crates/kernel/src/approval.rs` returns `Some` only for
`read_file` and `list_files`. A `write_file` approval is therefore
**non-resumable** — `/approval resume` never routes it to execution; this is a
structural property, not a no-op resume.

**Model tool-request parser:** The `CARAVAN_TOOL_REQUEST` / `ModelToolRequest`
parser accepts only `list_files` and `read_file`. Unsupported tool names —
including `write_file`, `shell_exec`, `apply_patch`, `delete_file`, and any network
tool — are rejected and never routed to execution. This parser rejection is the
hard backstop that "not model-visible" alone does not provide: even an adversarial
or accidental `CARAVAN_TOOL_REQUEST` block naming `write_file` is dropped. The
parser performs no filesystem I/O and no path canonicalization.

**Missing tools:** `shell_exec`, `apply_patch`, `delete_file`, and any network tool
do not exist.

**Sandbox:** No sandbox is implemented.

## Non-goals

The following are explicitly out of scope for this design baseline:

- Actual file write execution.
- Shell command execution (`shell_exec`).
- Patch application (`apply_patch`).
- File deletion (`delete_file`).
- Any network tool.
- Automatic model-driven mutation (the model cannot directly trigger a write).
- OpenAI function calling.
- MCP (Model Context Protocol) integration.
- Sandbox implementation (no sandbox exists yet).
- Background job or async mutation execution.
- Multi-agent tool delegation.

## Safety Principles

1. **Explicit user approval required for every mutation.** No mutation may occur
   without a human reviewing and approving the intent.
2. **Read-only tools may run without manual approval.** Reads carry no mutation
   risk and are pre-approved by policy.
3. **Mutation tools must never run directly from model output.** The model proposes;
   the user decides.
4. **The model may propose; the user must decide.** A model-generated write intent
   is not a trigger for execution.
5. **Approval and execution must both be recorded in the EventLog.** Observability
   of the full mutation lifecycle is mandatory.
6. **Path safety enforced at execution time.** The confinement check happens when
   the tool runs, not only at parse time.
7. **No secret values in events, logs, or errors.** Token values, API keys, and
   passwords must never appear in the EventLog.
8. **EventLog must not store full file content for large writes.** Summaries and
   bounded diffs only.
9. **`ToolResult` stores a summary, not the full mutation payload.** Detail fields
   hold counts and byte sizes, not raw file content.

## Workspace Confinement

All mutation paths must be workspace-root relative. The following are always
rejected:

- Absolute paths (e.g., `/etc/passwd`).
- `..` escape sequences that traverse above the workspace root.
- Canonicalized targets that resolve outside the workspace root.
- Symlink escapes (a symlink whose target points outside the workspace root).
- Parent-directory symlink escapes (a symlink in an ancestor directory that escapes
  the workspace root).

**Responsibility:** Path validation belongs to the **tool execution layer**. The
command parser does not canonicalize paths, and the approval projection does not
canonicalize paths. The execution layer revalidates the path every time a tool runs.

**Current state:** `resolve_in_workspace()` (in
`crates/kernel/src/tool/registry/path.rs`) enforces the confinement invariant for
**read tools today**. Future write tools **must reuse or adapt** the same function.

**Design requirement for writes (not yet implemented):** For a not-yet-existing
target (a create-new write), validate that the canonical form of the **parent
directory** plus the **final path component** stays inside the workspace root — so
a file that does not yet exist is still confined. This is a design requirement, not
a present protection for writes.

## Write File Policy

The following criteria apply to a future `write_file` implementation (none of this
exists today):

- `write_file` must require an `ApprovalRequest` before any I/O.
- `write_file` must **not** appear in the model-visible tool schema until a safe
  execution path exists.
- Explicit user approval is required; no implicit or automatic approval.
- Must not overwrite an existing file without policy — the first implementation may
  allow replacement only after explicit approval.
- First implementation creates a new file or replaces an existing one only after
  the user approves.
- The parent directory must exist unless an explicit create-parent policy is added
  later.
- Binary writes are out of scope; text-only UTF-8 in the first implementation.
- A maximum write size limit must exist and be documented before shipping.
- Newline handling (LF vs CRLF) must be documented per platform.
- Conservative file-permission handling: do not elevate permissions beyond the
  pre-existing file or platform default.

## Atomic Write Policy

The following principles apply to a future implementation (no implementation exists
today):

- Write to a temporary file under the **same directory** as the target, so the
  rename stays on the same filesystem.
- `fsync` the temporary file if practical; not required for the first POC.
- Rename atomically (where the OS guarantees it) to replace the target.
- Never write to the target file path directly first.
- Clean up the temporary file on any failure.
- Never leave partial content in the target if possible.
- Document cross-platform atomic-rename limitations (Windows does not guarantee
  `rename` atomicity for existing targets without `MoveFileEx` flags).

## Diff Preview Policy

The following principles apply to a future implementation (no implementation exists
today):

- Every write mutation has a preview or diff stage before the approval request.
- The `ApprovalRequest` event includes a **bounded** summary or diff (not the full
  file content for large files).
- The EventLog stores only a bounded diff summary; full content is never stored in
  `ToolResult`.
- `ToolResult` detail has no diff-content field today and should not receive one
  without a concrete implementation need.
- The initial diff format is line-based text (unified diff or similar).
- Binary diff is out of scope.
- Large diffs must be truncated to a documented maximum before storage.

## Backup / Rollback Policy

The following principles apply to a future implementation; no rollback exists today:

- The first write implementation **may choose to skip rollback** and must document
  that limitation explicitly.
- If replacing an existing file, a backup copy could be written to
  `.caravan/backups/` inside the workspace before overwrite (workspace-local,
  confined by the same path rules).
- A rollback command is out of scope for the first write POC.
- If a backup is taken, an event records a backup summary (path, byte size) — never
  the full content.
- Do not promise rollback before an implementation exists and is tested.

## Approval Flow

### Current flow (implemented today)

1. User runs `/tool plan-write <path>`.
2. `SlashCommand` event is logged (generic command log).
3. `ToolPolicy` event is recorded (policy evaluation for this tool and path).
4. `ApprovalRequest` event is recorded (mutation intent queued for review).
5. **No `ToolCall`, no `ToolResult`, no file write.** The handler returns
   `Err(ToolError::ApprovalRequired)` immediately.
6. User runs `/approval approve` → `ApprovalDecision` event recorded, no `ToolCall`.
7. User runs `/approval resume` for `write_file` → **cannot resume**: `to_tool_request()`
   returns `None` for `write_file`, so no `ApprovalResumePlan` is produced and
   execution is never reached.

### Future flow (not yet implemented)

1. User or model produces a write intent.
2. `ToolPolicy` event recorded (policy gate).
3. `ApprovalRequest` event recorded (intent queued).
4. User approves → `ApprovalDecision` event recorded.
5. User triggers resume → `ApprovalResume` event recorded.
6. Policy is **rechecked** at resume time; path is **revalidated** at resume time.
7. `ToolCall` event recorded (execution begins).
8. `ToolResult` or `ToolError` event recorded (execution outcome).

**Approval does not imply automatic execution.** Resume is explicit and initiated
by the user. Policy is rechecked and the path is revalidated at resume time, not
only at approval time.

## Event Design

### Current boundary events in use

| Event Kind        | Role                                                        |
|-------------------|-------------------------------------------------------------|
| `SlashCommand`    | Logged for **every** slash command (command logging, not a tool/approval boundary event). |
| `ToolPolicy`      | Records the policy evaluation result for a specific tool.   |
| `ApprovalRequest` | Records mutation intent queued for user review.             |
| `ApprovalDecision`| Records the user's approve or reject decision.              |
| `ApprovalResume`  | Records that the user triggered resume execution.           |
| `ToolCall`        | Records that a tool began executing.                        |
| `ToolResult`      | Records a successful tool execution summary.                |
| `ToolError`       | Records a tool execution failure.                           |

### Notes on `write_file`

`write_file` appears only as a **tool-name string value** inside existing event
detail payloads (e.g., in `ToolPolicy` or `ApprovalRequest` detail). It is **not**
an `EventKind` variant.

### New EventKind additions

No new `EventKind` variant is required for the write POC. Specifically:

- Do not add `WritePreview` until a real implementation needs it.
- Do not add separate `ApprovalApprove` / `ApprovalReject` variants; `ApprovalDecision`
  already captures both outcomes.

## Model Interaction Policy

- The model must **not** directly execute a mutation.
- The model may propose a tool-request block only for currently supported
  detect-only or read-only paths.
- `write_file` is **not** model-visible today.
- `write_file` must **not** be added to the Available Tools prompt section until
  safe execution is implemented and reviewed.
- The `ModelToolRequest` parser rejects `write_file` (and every other unsupported
  tool name); only `list_files` and `read_file` are accepted. "Not model-visible"
  covers the prompt/schema surface; parser rejection is the separate hard backstop
  against an unsupported `CARAVAN_TOOL_REQUEST` block reaching execution.
- OpenAI function calling is not enabled.
- MCP (Model Context Protocol) is not enabled.
- Automatic tool execution from model output is not enabled.

## Future POC Roadmap

Recommended implementation order:

1. **WriteIntent data model** — define the in-memory struct for a write request
   (path, content, risk level, reason) with no I/O.
2. **`write_file` dry-run / diff preview only** — compute and display the diff
   without writing anything.
3. **Approval request with diff summary** — record an `ApprovalRequest` that
   includes a bounded diff summary in its detail.
4. **Approval resume executes text write with atomic write** — implement
   `to_tool_request()` for `write_file`, atomic temp-rename write, and record
   `ToolCall` + `ToolResult`.
5. **Rollback / backup policy** — decide whether `.caravan/backups/` is
   implemented; document the decision.
6. **Model-visible mutation request** — add `write_file` to the available-tools
   prompt only after safety is proven in steps 1–5.
7. **Sandbox expansion** — explore OS-level sandbox (e.g., `seccomp`, macOS
   sandbox profiles) around the write executor.
8. **`shell_exec`** — only after all write-tool safety lessons are applied; this
   carries significantly higher risk and is deferred.

## Hard Safety Invariants

- No mutation without explicit user approval.
- No mutation directly from model output.
- No write outside the workspace root.
- No symlink escape.
- No secret value in the EventLog.
- No unbounded content in `ToolResult`.
- Revalidate path at execution time (not only at parse or approval time).
- Recheck policy at resume time (policy must still be satisfied when execution is
  triggered, not only when the request was first created).
- Prefer atomic write (temp file + rename) over direct target write.
- Keep `write_file` out of the model-visible tool schema until the implementation
  is safe and reviewed.
- Keep `write_file` rejected by the model tool-request parser until a safe
  implementation is reviewed.
