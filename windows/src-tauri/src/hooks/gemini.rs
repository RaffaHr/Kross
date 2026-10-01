// Gemini CLI — ~/.gemini/settings.json.
//
// Gemini's hook schema wraps each entry in a matcher block and gives every
// hook a name: `{matcher:"*", hooks:[{name:"coucou", type:"command",
// command:"..."}]}`. Timeouts are milliseconds here, not seconds — the spec
// values are multiplied at render time.
use super::{CliSpec, EntryShape, SpecFile};

const EVENTS: &[(&str, u64)] = &[
    ("SessionStart", 10),
    ("SessionEnd", 10),
    ("BeforeAgent", 10),
    ("AfterAgent", 10),
    ("BeforeTool", 10),
    ("AfterTool", 10),
    ("BeforeModel", 10),
    ("AfterModel", 10),
];

pub const SPEC: CliSpec = CliSpec {
    provider: "google",
    cli: "Gemini CLI",
    dir: ".gemini",
    binary: "gemini",
    files: &[SpecFile::Hooks(".gemini/settings.json", EntryShape::Gemini)],
    events: EVENTS,
    note: None,
};
