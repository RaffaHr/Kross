// Codex CLI — ~/.codex/hooks.json plus `[features] hooks = true` in config.toml.
//
// Codex's hook schema mirrors Claude's entries (`{hooks:[{type,command,
// timeout}]}` under a "hooks" map), but the feature ships off by default —
// the TOML flag is what turns it on. Codex also puts newly installed hooks
// through a trust review (`/hooks` in the CLI) before it runs them; the note
// tells the user that instead of leaving them wondering why nothing fired.
use super::{CliSpec, EntryShape, SpecFile};

const EVENTS: &[(&str, u64)] = &[
    ("SessionStart", 10),
    ("SessionEnd", 10),
    ("UserPromptSubmit", 10),
    ("PreToolUse", 10),
    ("PermissionRequest", 120),
    ("PostToolUse", 10),
    ("PreCompact", 10),
    ("PostCompact", 10),
    ("SubagentStart", 10),
    ("SubagentStop", 10),
    ("Stop", 10),
];

pub const SPEC: CliSpec = CliSpec {
    provider: "codex",
    cli: "Codex CLI",
    dir: ".codex",
    binary: "codex",
    files: &[
        SpecFile::Hooks(".codex/hooks.json", EntryShape::CliLike),
        SpecFile::CodexFeatures(".codex/config.toml"),
    ],
    events: EVENTS,
    note: Some(
        "Codex reviews new hooks before running them — approve Coucou's entries once with /hooks inside the CLI.",
    ),
};
