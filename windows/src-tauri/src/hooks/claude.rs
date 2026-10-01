// Claude Code — ~/.claude/settings.json, the original surface.
use super::{CliSpec, EntryShape, SpecFile};

/// Every event the island reacts to, with the hook timeout written to
/// settings.json. PermissionRequest waits for a human, so it gets the decision
/// timeout + 10 s.
const EVENTS: &[(&str, u64)] = &[
    ("SessionStart", 10),
    ("SessionEnd", 10),
    ("UserPromptSubmit", 10),
    ("PreToolUse", 10),
    ("PostToolUse", 10),
    ("PostToolUseFailure", 10),
    ("PermissionRequest", 120),
    ("Notification", 10),
    ("Stop", 10),
    ("StopFailure", 10),
    ("SubagentStart", 10),
    ("SubagentStop", 10),
];

pub const SPEC: CliSpec = CliSpec {
    provider: "claude",
    cli: "Claude Code",
    dir: ".claude",
    binary: "claude",
    files: &[SpecFile::Hooks(".claude/settings.json", EntryShape::CliLike)],
    events: EVENTS,
    note: None,
};
