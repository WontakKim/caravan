#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum CommandHelpSurface {
    Default,
    InternalHarness,
}

pub struct CommandHelpEntry {
    pub command: &'static str,
    pub description: &'static str,
}

pub struct HelpSection {
    pub header: &'static str,
    pub entries: &'static [CommandHelpEntry],
    pub surface: CommandHelpSurface,
}

static HELP_SECTIONS: &[HelpSection] = &[
    HelpSection {
        header: "Claude-like core commands",
        surface: CommandHelpSurface::Default,
        entries: &[
            CommandHelpEntry {
                command: "/help",
                description: "show this help",
            },
            CommandHelpEntry {
                command: "/clear",
                description: "clear the log",
            },
            CommandHelpEntry {
                command: "/exit",
                description: "exit Caravan",
            },
            CommandHelpEntry {
                command: "/reset",
                description: "reset the session (clears screen log and pending state)",
            },
            CommandHelpEntry {
                command: "/new",
                description: "start a new session (alias for /reset)",
            },
            CommandHelpEntry {
                command: "/quit",
                description: "quit Caravan (alias for /exit)",
            },
            CommandHelpEntry {
                command: "/permissions",
                description: "show the current permission posture",
            },
            CommandHelpEntry {
                command: "/allowed-tools",
                description: "list the tools that are currently allowed",
            },
        ],
    },
    HelpSection {
        header: "Basic workspace tools",
        surface: CommandHelpSurface::Default,
        entries: &[
            CommandHelpEntry {
                command: "/tool list [path]",
                description: "list files under the workspace",
            },
            CommandHelpEntry {
                command: "/tool read <path> [--offset <line>] [--limit <lines>]",
                description: "read a UTF-8 text file under the workspace",
            },
            CommandHelpEntry {
                command: "/tool search <query>",
                description: "search for a string across workspace files",
            },
            CommandHelpEntry {
                command: "/tool glob <pattern>",
                description: "find files matching a glob pattern in the workspace",
            },
        ],
    },
    HelpSection {
        header: "Workspace context commands",
        surface: CommandHelpSurface::InternalHarness,
        entries: &[
            CommandHelpEntry {
                command: "/context attach-last-tool",
                description: "attach the latest read-only tool output to the next prompt",
            },
            CommandHelpEntry {
                command: "/context clear",
                description: "clear pending manual tool context",
            },
            CommandHelpEntry {
                command: "/context status",
                description: "show pending manual tool context and last tool output",
            },
        ],
    },
    HelpSection {
        header: "Advanced experimental harness commands",
        surface: CommandHelpSurface::InternalHarness,
        entries: &[
            CommandHelpEntry {
                command: "/request status",
                description: "show the pending model tool request",
            },
            CommandHelpEntry {
                command: "/request clear",
                description: "clear the pending model tool request",
            },
            CommandHelpEntry {
                command: "/request run",
                description: "execute the pending model tool request (read-only)",
            },
            CommandHelpEntry {
                command: "/approval status",
                description: "show pending approval requests",
            },
            CommandHelpEntry {
                command: "/approval approve <seq>",
                description: "approve a pending approval request",
            },
            CommandHelpEntry {
                command: "/approval reject <seq>",
                description: "reject a pending approval request",
            },
            CommandHelpEntry {
                command: "/approval resume <seq>",
                description: "resume an approved read-only tool plan (consumed on attempt)",
            },
        ],
    },
    HelpSection {
        header: "Write/sandbox experimental commands",
        surface: CommandHelpSurface::InternalHarness,
        entries: &[
            CommandHelpEntry {
                command: "/tool plan-write <path>",
                description: "approval-only skeleton: records workspace_write intent (ToolPolicy + ApprovalRequest) without writing any file",
            },
            CommandHelpEntry {
                command: "/tool preview-write <path>",
                description: "read-only dry-run diff preview of a proposed write using the latest tool output as content; performs no write",
            },
            CommandHelpEntry {
                command: "/tool propose-write <path>",
                description: "preview-backed approval request: shows a bounded diff preview and records a workspace_write ApprovalRequest using the latest tool output as content; performs no write",
            },
        ],
    },
];

pub fn command_help_sections() -> &'static [HelpSection] {
    HELP_SECTIONS
}

pub fn default_command_help_sections() -> Vec<&'static HelpSection> {
    HELP_SECTIONS
        .iter()
        .filter(|s| s.surface == CommandHelpSurface::Default)
        .collect()
}

pub fn command_help_entries() -> Vec<&'static CommandHelpEntry> {
    HELP_SECTIONS
        .iter()
        .flat_map(|s| s.entries.iter())
        .collect()
}
