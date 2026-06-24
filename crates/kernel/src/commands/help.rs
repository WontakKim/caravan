pub struct CommandHelpEntry {
    pub command: &'static str,
    pub description: &'static str,
}

static HELP_ENTRIES: &[CommandHelpEntry] = &[
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
        command: "/tool list [path]",
        description: "list files under the workspace",
    },
    CommandHelpEntry {
        command: "/tool read <path>",
        description: "read a UTF-8 text file under the workspace",
    },
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
];

pub fn command_help_entries() -> &'static [CommandHelpEntry] {
    HELP_ENTRIES
}
