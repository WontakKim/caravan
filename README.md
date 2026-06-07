# Caravan

A minimal Rust TUI shell skeleton. Agents, models, and tools are out of scope for this POC.

## Running

```sh
cargo run
```

## Available Commands

| Command   | Description                          |
|-----------|--------------------------------------|
| `/help`   | Show the list of available commands  |
| `/clear`  | Clear the log panel                  |
| `/exit`   | Exit the application                 |

Any other input is echoed to the Log panel.

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
Log. Each navigation step appends an `InspectorSelectionChanged` event to the
log recording the newly selected `seq`.

- **Down** — move to the next (newer) event; no-op at the bottom boundary.
- **Up** — move to the previous (older) event; no-op at the top boundary.

### EventKind Values

| EventKind                    | When it is recorded                                      |
|------------------------------|----------------------------------------------------------|
| `AppStarted`                 | Once, when the application initialises                   |
| `CommandEntered`             | Every time Enter is pressed with non-empty input         |
| `HelpRequested`              | When `/help` is processed                                |
| `UserTextEntered`            | When plain (non-command) text is submitted               |
| `LogCleared`                 | When `/clear` is processed                               |
| `InspectorSelectionChanged`  | Each time the Up/Down selection changes                  |
| `ExitRequested`              | When `/exit` is processed or Ctrl+C is pressed           |
| `UnknownCommand`             | When an unrecognised `/command` is entered               |

### `/clear` Behaviour

`/clear` empties the **screen log** (the Main panel history) but does **not**
clear the Event Log. The Event Log is append-only; there is no mechanism to
remove events once they have been recorded. `/clear` also does **not** delete
or truncate the on-disk `.caravan/events.jsonl` file — persisted events remain
intact across restarts.

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

## Manual Verification

The following checks must be confirmed interactively before the POC is considered done:

- [ ] `cargo run` opens the TUI showing the Header (`Caravan | TUI Shell | Status: Ready`),
      the Nav/Main/Inspector columns, the Log panel, and the Command Bar — without panicking.
- [ ] Typing plain text then pressing Enter appends that text to the Log and clears the
      Command Bar; the Main panel stays on the static welcome screen.
- [ ] `/help` appends the command list to the Log only; Main panel is unchanged.
- [ ] `/clear` empties the Log panel; the Event Log retains all previous entries.
- [ ] An unknown command (e.g. `/foo`) appends an `Unknown command:` line to the Log.
- [ ] `/exit` exits the app cleanly and the terminal is fully restored (no raw-mode residue,
      cursor and normal screen returned).
- [ ] Pressing Down selects the first event; Inspector shows its seq, kind, and message.
- [ ] Pressing Up and Down navigates between events; the selected row is highlighted in the
      Event Log panel.
