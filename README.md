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
| `/quit`   | Exit the application                 |

Any other input is echoed to the Log panel.

## Manual Verification

The following checks must be confirmed interactively before the POC is considered done:

- [ ] `cargo run` opens the TUI showing the Header (`Caravan | TUI Shell | Status: Ready`),
      the Nav/Main/Inspector columns, the Log panel, and the Command Bar — without panicking.
- [ ] Typing plain text then pressing Enter appends that text to the Log and clears the
      Command Bar; the Main panel stays on the static welcome screen.
- [ ] `/help` appends the command list to the Log only; Main panel is unchanged.
- [ ] `/clear` empties the Log panel.
- [ ] An unknown command (e.g. `/foo`) appends an `Unknown command:` line to the Log.
- [ ] `/quit` exits the app cleanly and the terminal is fully restored (no raw-mode residue,
      cursor and normal screen returned).
