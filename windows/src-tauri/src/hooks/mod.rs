// Hook installation for the CLIs Coucou can ride on.
//
// Same rule as always, now applied per provider: read the CLI's own config,
// take a dated backup, merge without touching anybody else's hooks, show the
// diff, and write only after an explicit click. Uninstall removes Coucou's
// entries and nothing else.
//
// Each CLI describes itself as a `CliSpec`: which files it manages, which
// events it understands, and the entry shape its config schema expects. The
// command is only the quoted exe path in forward slashes plus arguments —
// on Windows the CLIs run hook commands through shells, and anything with
// PowerShell or cmd quoting in it breaks.
//
//   claude → ~/.claude/settings.json   (existing schema, unchanged commands)
//   codex  → ~/.codex/hooks.json       (same entries; command gets a "codex"
//                                       provider arg) plus ~/.codex/config.toml
//                                       `[features] hooks = true`
//   gemini → ~/.gemini/settings.json   (matcher + named hook entries)

mod claude;
mod codex;
mod gemini;

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Map, Value};
use tauri::{AppHandle, Manager};
use windows::Win32::System::SystemInformation::GetLocalTime;

use crate::settings;

/// Marker that identifies a Coucou entry inside a CLI's hook list.
const MARKER: &str = "coucou-hook";

/// How a JSON hooks file spells a single entry. Claude and Codex share the
/// `{hooks: [{type, command, timeout}]}` shape; Gemini wraps hooks in a
/// matcher block with a `name` on each hook.
#[derive(Clone, Copy)]
pub enum EntryShape {
    CliLike,
    Gemini,
}

/// One file a CLI spec owns (partially — foreign content is always preserved).
#[derive(Clone, Copy)]
pub enum SpecFile {
    /// JSON file whose top-level "hooks" object maps event → entries.
    Hooks(&'static str, EntryShape),
    /// Codex's config.toml — we only ensure `[features] hooks = true`.
    CodexFeatures(&'static str),
}

/// Everything Coucou needs to know about a CLI's hook surface.
pub struct CliSpec {
    /// The provider id this CLI belongs to.
    pub provider: &'static str,
    /// Display name for the UI ("Claude Code", "Codex CLI", "Gemini CLI").
    pub cli: &'static str,
    /// Config directory under %USERPROFILE% used for CLI detection.
    pub dir: &'static str,
    /// Binary name for PATH detection (`where codex` etc.).
    pub binary: &'static str,
    /// Files the spec manages, relative to %USERPROFILE%.
    pub files: &'static [SpecFile],
    /// Supported events and the hook timeout (seconds) written for each.
    /// `PermissionRequest` waits for a human, so it gets the decision
    /// timeout + 10 s.
    pub events: &'static [(&'static str, u64)],
    /// Shown under the hooks panel — trust reviews, flags the CLI still needs.
    pub note: Option<&'static str>,
}

static SPECS: &[CliSpec] = &[claude::SPEC, codex::SPEC, gemini::SPEC];

/// The CLI spec a provider's hooks ride on, if it has one.
pub fn spec_for(provider: &str) -> Option<&'static CliSpec> {
    SPECS.iter().find(|s| s.provider == provider)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookStatus {
    pub installed: bool,
    pub cli: String,
    /// False when neither the config dir nor the binary can be found.
    pub cli_detected: bool,
    pub note: Option<String>,
    /// Events this CLI supports, for the UI's event list.
    pub events: Vec<String>,
    pub settings_path: String,
    pub hook_path: String,
    pub hook_ready: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookPreview {
    pub diff: String,
    pub backup: String,
    pub settings_path: String,
    /// Identifies the bytes this diff was computed from; handed back to `write`
    /// so we only ever apply what the user actually looked at.
    pub fingerprint: String,
}

fn home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

impl CliSpec {
    /// Every managed file, resolved under %USERPROFILE%.
    fn file_paths(&self) -> Vec<PathBuf> {
        self.files
            .iter()
            .map(|f| match f {
                SpecFile::Hooks(rel, _) | SpecFile::CodexFeatures(rel) => home().join(rel),
            })
            .collect()
    }

    /// The hooks-carrying file — the one the UI calls "settings".
    pub fn hooks_path(&self) -> PathBuf {
        self.files
            .iter()
            .find_map(|f| match f {
                SpecFile::Hooks(rel, _) => Some(home().join(rel)),
                _ => None,
            })
            .unwrap_or_else(|| self.file_paths()[0].clone())
    }

    /// The CLI looks present when its config dir exists or the binary is on PATH.
    fn detected(&self) -> bool {
        home().join(self.dir).is_dir() || binary_on_path(self.binary)
    }
}

/// `where` for a binary name, checking the extensions Windows actually launches.
fn binary_on_path(name: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                ["exe", "cmd", "bat", ""].iter().any(|ext| {
                    let file = if ext.is_empty() {
                        dir.join(name)
                    } else {
                        dir.join(format!("{name}.{ext}"))
                    };
                    file.is_file()
                })
            })
        })
        .unwrap_or(false)
}

/// Reads a JSON file.
///
/// The only error that means "start from nothing" is the file not being there.
/// Everything else — a lock held by another process, a permission problem, JSON
/// we cannot parse — is reported, because the alternative is treating somebody's
/// unreadable settings as an empty object and then writing that back over them.
fn read_json(path: &Path) -> Result<Value, String> {
    match std::fs::read(path) {
        Ok(bytes) => parse_json(&bytes, &path.display().to_string()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        // A lock, a permission problem, a bad drive: all of them mean we do not
        // know what is in there, and not knowing is not the same as empty.
        Err(err) => Err(format!("Can't read {}: {err}", path.display())),
    }
}

/// The parsing half of `read_json`, split out so it can be tested without a
/// home directory.
fn parse_json(bytes: &[u8], path: &str) -> Result<Value, String> {
    // PowerShell writes a UTF-8 BOM with `Set-Content -Encoding utf8`, and
    // serde_json refuses it. Stripping it is safe and well defined; guessing at
    // anything else is not.
    let text = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    if text.iter().all(u8::is_ascii_whitespace) {
        return Ok(json!({}));
    }
    match serde_json::from_slice::<Value>(text) {
        Ok(v) if v.is_object() => Ok(v),
        Ok(_) => Err(format!("{path} isn't a JSON object — Coucou won't touch it.")),
        Err(err) => Err(format!(
            "{path} isn't valid JSON ({err}). Fix or move it, then try again — Coucou won't overwrite it."
        )),
    }
}

/// The settings as they are, or an empty object when we cannot tell. Only for
/// read-only paths like `status()`, which must never fail loudly; anything that
/// writes uses `read_json()` and surfaces the error instead.
fn read_json_lossy(path: &Path) -> Value {
    read_json(path).unwrap_or_else(|_| json!({}))
}

fn hook_command(spec: &CliSpec, event: &str) -> String {
    let exe = settings::hook_exe_path().to_string_lossy().replace('\\', "/");
    // Claude's installed base predates the provider arg and stays compatible
    // without it; the newer CLIs get `coucou-hook <provider> <event>`.
    if spec.provider == "claude" {
        format!("\"{exe}\" {event}")
    } else {
        format!("\"{exe}\" {} {event}", spec.provider)
    }
}

fn make_entry(spec: &CliSpec, shape: EntryShape, event: &str, timeout: u64) -> Value {
    let command = hook_command(spec, event);
    match shape {
        EntryShape::CliLike => json!({
            "hooks": [{
                "type": "command",
                "command": command,
                "timeout": timeout,
            }]
        }),
        EntryShape::Gemini => json!({
            "matcher": "*",
            "hooks": [{
                "name": "coucou",
                "type": "command",
                "command": command,
                "timeout": timeout * 1000,
            }]
        }),
    }
}

fn entry_is_ours(entry: &Value) -> bool {
    entry
        .get("hooks")
        .and_then(Value::as_array)
        .map(|hooks| {
            hooks.iter().any(|h| {
                h.get("command")
                    .and_then(Value::as_str)
                    .map(|c| c.contains(MARKER))
                    .unwrap_or(false)
            })
        })
        .unwrap_or(false)
}

/// Hooks JSON with Coucou's entries added; everything else is left untouched.
fn merged(spec: &CliSpec, shape: EntryShape, existing: &Value) -> Value {
    let mut root = existing.as_object().cloned().unwrap_or_default();
    let mut hooks = root
        .get("hooks")
        .and_then(Value::as_object)
        .cloned()
        .unwrap_or_else(Map::new);

    for (event, timeout) in spec.events {
        let mut list = hooks
            .get(*event)
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        list.retain(|entry| !entry_is_ours(entry));
        list.push(make_entry(spec, shape, event, *timeout));
        hooks.insert((*event).to_string(), Value::Array(list));
    }

    root.insert("hooks".into(), Value::Object(hooks));
    Value::Object(root)
}

/// Hooks JSON with every Coucou entry removed, and nothing else changed.
fn without_ours(existing: &Value) -> Value {
    let mut root = existing.as_object().cloned().unwrap_or_default();
    let Some(hooks) = root.get("hooks").and_then(Value::as_object).cloned() else {
        return Value::Object(root);
    };
    let mut out = Map::new();
    for (event, value) in hooks {
        match value.as_array() {
            Some(list) => {
                let kept: Vec<Value> =
                    list.iter().filter(|e| !entry_is_ours(e)).cloned().collect();
                if !kept.is_empty() {
                    out.insert(event, Value::Array(kept));
                }
            }
            None => {
                out.insert(event, value);
            }
        }
    }
    if out.is_empty() {
        root.remove("hooks");
    } else {
        root.insert("hooks".into(), Value::Object(out));
    }
    Value::Object(root)
}

/// Codex's config.toml with `[features] hooks = true` ensured. Everything else
/// in the file — profiles, model settings, other features — survives verbatim
/// because we edit the parsed document, not the text.
fn with_codex_hooks_enabled(existing: &str) -> Result<String, String> {
    let mut doc = existing
        .parse::<toml_edit::DocumentMut>()
        .map_err(|e| format!("config.toml isn't valid TOML ({e}) — Coucou won't touch it."))?;
    doc["features"]["hooks"] = toml_edit::value(true);
    Ok(doc.to_string())
}

fn pretty(v: &Value) -> String {
    serde_json::to_string_pretty(v).unwrap_or_default()
}

/// Down to the second: installing then uninstalling in the same minute must not
/// quietly overwrite the first backup.
fn stamp() -> String {
    let t = unsafe { GetLocalTime() };
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        t.wYear, t.wMonth, t.wDay, t.wHour, t.wMinute, t.wSecond
    )
}

fn backup_path(path: &Path) -> PathBuf {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("settings.json");
    path.with_file_name(format!("{name}.bak-{}", stamp()))
}

/// Identifies the exact bytes a preview was computed from. FNV-1a is plenty:
/// the question is only "is this still the file I showed the user?".
fn fingerprint_bytes(bytes: &[u8]) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= *b as u64;
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:016x}")
}

/// One fingerprint over every file the spec manages, so a preview is stale if
/// *any* of them moved.
fn current_fingerprint(spec: &CliSpec) -> String {
    let mut bytes = Vec::new();
    for path in spec.file_paths() {
        if let Ok(b) = std::fs::read(&path) {
            bytes.extend_from_slice(&b);
        }
        bytes.push(0x1F); // unit separator between files
    }
    fingerprint_bytes(&bytes)
}

/// (path, next text) for every managed file, after the install or uninstall.
fn render(spec: &CliSpec, install: bool) -> Result<Vec<(PathBuf, String)>, String> {
    let mut out = Vec::new();
    for file in spec.files {
        match file {
            SpecFile::Hooks(rel, shape) => {
                let path = home().join(rel);
                let current = read_json(&path)?;
                let next = if install {
                    merged(spec, *shape, &current)
                } else {
                    without_ours(&current)
                };
                let mut text = pretty(&next);
                text.push('\n');
                out.push((path, text));
            }
            SpecFile::CodexFeatures(rel) => {
                // Install-only: enabling the flag is additive and reversible,
                // and other people's Codex hooks may depend on it — so an
                // uninstall never turns it back off.
                if !install {
                    continue;
                }
                let path = home().join(rel);
                let current = std::fs::read_to_string(&path)
                    .or_else(|e| {
                        if e.kind() == std::io::ErrorKind::NotFound {
                            Ok(String::new())
                        } else {
                            Err(format!("Can't read {}: {e}", path.display()))
                        }
                    })?;
                out.push((path, with_codex_hooks_enabled(&current)?));
            }
        }
    }
    Ok(out)
}

// ── Public API ────────────────────────────────────────────────────────────────

impl CliSpec {
    pub fn status(&self) -> HookStatus {
        let installed = self.files.iter().any(|f| match f {
            SpecFile::Hooks(rel, _) => read_json_lossy(&home().join(rel))
                .get("hooks")
                .and_then(Value::as_object)
                .map(|hooks| {
                    hooks
                        .values()
                        .filter_map(Value::as_array)
                        .flatten()
                        .any(entry_is_ours)
                })
                .unwrap_or(false),
            _ => false,
        });
        let hook_path = settings::hook_exe_path();
        HookStatus {
            installed,
            cli: self.cli.to_string(),
            cli_detected: self.detected(),
            note: self.note.map(str::to_string),
            events: self.events.iter().map(|(e, _)| (*e).to_string()).collect(),
            settings_path: self.hooks_path().to_string_lossy().to_string(),
            hook_ready: hook_path.exists(),
            hook_path: hook_path.to_string_lossy().to_string(),
        }
    }

    pub fn preview(&self, install: bool) -> Result<HookPreview, String> {
        let rendered = render(self, install)?;
        let mut diff = String::new();
        let mut backups = Vec::new();
        for (path, next) in &rendered {
            let current = std::fs::read_to_string(path).unwrap_or_default();
            if rendered.len() > 1 {
                diff.push_str(&format!("── {} ──\n", path.display()));
            }
            diff.push_str(&unified_diff(&current, next));
            diff.push('\n');
            backups.push(backup_path(path).to_string_lossy().to_string());
        }
        Ok(HookPreview {
            diff,
            backup: backups.join("\n"),
            settings_path: self.hooks_path().to_string_lossy().to_string(),
            fingerprint: current_fingerprint(self),
        })
    }

    /// Writes the rendered files after taking dated backups of every one.
    ///
    /// `fingerprint` is the one the preview was computed from. If any managed
    /// file changed in between — another tool, another window, the user's own
    /// editor — we stop and make them look at a fresh diff, because the only
    /// thing worse than not installing the hooks is silently reverting
    /// somebody else's edit.
    pub fn write(&self, install: bool, fingerprint: &str) -> Result<String, String> {
        let rendered = render(self, install)?;
        for (path, _) in &rendered {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
        }

        // Read before the backup: an unreadable file must abort before we touch
        // anything at all.
        if current_fingerprint(self) != fingerprint {
            return Err(format!(
                "{} changed since the preview. Nothing was written — review the new diff.",
                self.hooks_path().display()
            ));
        }

        let mut backups = Vec::new();
        for (path, text) in rendered {
            let backup = backup_path(&path);
            if path.exists() {
                std::fs::copy(&path, &backup)
                    .map_err(|e| format!("backup failed: {e}"))?;
            }
            // Write beside the target and rename over it: a crash or a full
            // disk leaves the original file intact rather than half a file.
            let temp = path.with_extension(format!(
                "coucou-{}",
                std::process::id()
            ));
            std::fs::write(&temp, text.as_bytes())
                .map_err(|e| format!("write failed: {e}"))?;
            if let Err(err) = std::fs::rename(&temp, &path) {
                let _ = std::fs::remove_file(&temp);
                return Err(format!("write failed: {err}"));
            }
            backups.push(backup.to_string_lossy().to_string());
        }
        Ok(backups.join("\n"))
    }
}

/// Copies coucou-hook.exe into %LOCALAPPDATA%\Coucou\bin on launch.
/// In a bundled install it comes from the app resources; in `tauri dev` it sits
/// next to coucou.exe in the workspace target directory.
///
/// Every candidate is tried rather than just the first, because getting this
/// wrong is silent and fatal: `resources` used to be a glob, which made NSIS
/// mirror the source path into `_up_\target\release\`, no candidate matched, and
/// the relay was simply never installed. It only looked healthy on a developer
/// machine, where a leftover copy from `tauri dev` was already sitting in bin/.
pub fn ensure_hook_exe(app: &AppHandle) {
    let dest = settings::hook_exe_path();
    let Some(dir) = dest.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }

    let mut candidates: Vec<PathBuf> = Vec::new();
    if let Ok(p) = app.path().resolve("coucou-hook.exe", tauri::path::BaseDirectory::Resource) {
        candidates.push(p);
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            // Installed build, then `tauri dev` (target/debug) next to the
            // release hook the pre-build step produces.
            candidates.push(parent.join("coucou-hook.exe"));
            candidates.push(parent.join("../release/coucou-hook.exe"));
            // Belt and braces: where the old glob form used to land it.
            candidates.push(parent.join("_up_/target/release/coucou-hook.exe"));
        }
    }

    let tried: Vec<String> = candidates.iter().map(|p| p.display().to_string()).collect();
    let Some(src) = candidates.into_iter().find(|p| p.exists()) else {
        crate::log::line(format!(
            "coucou-hook.exe not found — hooks cannot work. Looked in: {}",
            tried.join(", ")
        ));
        return;
    };

    let same = match (std::fs::metadata(&src), std::fs::metadata(&dest)) {
        (Ok(a), Ok(b)) => a.len() == b.len() && a.modified().ok() == b.modified().ok(),
        _ => false,
    };
    if same {
        return;
    }
    // A hook may be running right now and hold the file open; keeping the old
    // copy is fine, it is the same relay.
    if let Err(err) = std::fs::copy(&src, &dest) {
        if !dest.exists() {
            crate::log::line(format!("could not install coucou-hook.exe: {err}"));
        }
    }
}

// ── Minimal unified diff (LCS) ────────────────────────────────────────────────

/// The configs are short, so a plain O(n·m) LCS is the simplest honest diff.
fn unified_diff(before: &str, after: &str) -> String {
    let a: Vec<&str> = before.lines().collect();
    let b: Vec<&str> = after.lines().collect();
    let (n, m) = (a.len(), b.len());

    let mut lcs = vec![vec![0usize; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if a[i] == b[j] {
                lcs[i + 1][j + 1] + 1
            } else {
                lcs[i + 1][j].max(lcs[i][j + 1])
            };
        }
    }

    let mut out: Vec<String> = Vec::new();
    let (mut i, mut j) = (0usize, 0usize);
    while i < n && j < m {
        if a[i] == b[j] {
            out.push(format!("  {}", a[i]));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            out.push(format!("- {}", a[i]));
            i += 1;
        } else {
            out.push(format!("+ {}", b[j]));
            j += 1;
        }
    }
    while i < n {
        out.push(format!("- {}", a[i]));
        i += 1;
    }
    while j < m {
        out.push(format!("+ {}", b[j]));
        j += 1;
    }

    // Keep three lines of context around each change so the panel stays readable.
    let changed: Vec<usize> = out
        .iter()
        .enumerate()
        .filter(|(_, l)| l.starts_with('+') || l.starts_with('-'))
        .map(|(i, _)| i)
        .collect();
    if changed.is_empty() {
        return "No change.".into();
    }
    let mut keep = vec![false; out.len()];
    for idx in changed {
        let lo = idx.saturating_sub(3);
        let hi = (idx + 4).min(out.len());
        keep[lo..hi].fill(true);
    }
    let mut result = String::new();
    let mut gap = false;
    for (idx, line) in out.iter().enumerate() {
        if keep[idx] {
            result.push_str(line);
            result.push('\n');
            gap = false;
        } else if !gap {
            result.push_str("  …\n");
            gap = true;
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    const WHERE: &str = "settings.json";

    #[test]
    fn a_utf8_bom_is_stripped_not_treated_as_corruption() {
        // PowerShell 5's `Set-Content -Encoding utf8` produces exactly this.
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(br#"{"model":"opus","hooks":{}}"#);
        let parsed = parse_json(&bytes, WHERE).expect("a BOM must not defeat the parser");
        assert_eq!(parsed["model"], "opus");
    }

    #[test]
    fn unreadable_content_is_an_error_never_an_empty_object() {
        // This is the whole bug: returning {} here meant `merged()` produced a
        // file containing nothing but Coucou's hooks, and the write replaced
        // everything the user had.
        for bad in [&b"{ not json"[..], &b"[1,2,3]"[..], &b"\"a string\""[..]] {
            assert!(
                parse_json(bad, WHERE).is_err(),
                "content we cannot use must refuse, not come back empty"
            );
        }
    }

    #[test]
    fn empty_and_whitespace_files_start_from_nothing() {
        assert_eq!(parse_json(b"", WHERE).unwrap(), json!({}));
        assert_eq!(parse_json(b"  \n\t ", WHERE).unwrap(), json!({}));
    }

    #[test]
    fn merging_keeps_every_other_setting_and_every_foreign_hook() {
        let existing = serde_json::json!({
            "model": "claude-opus-5",
            "theme": "dark",
            "enabledPlugins": ["a", "b"],
            "hooks": {
                "PreToolUse": [
                    { "hooks": [{ "type": "command", "command": "someone-elses-tool.exe" }] }
                ],
                "SomeEventWeDoNotTouch": [
                    { "hooks": [{ "type": "command", "command": "keep-me.exe" }] }
                ]
            }
        });

        let after = merged(&claude::SPEC, EntryShape::CliLike, &existing);
        assert_eq!(after["model"], "claude-opus-5");
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["enabledPlugins"], serde_json::json!(["a", "b"]));

        let pre = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(
            pre.iter().any(|e| serde_json::to_string(e).unwrap().contains("someone-elses-tool.exe")),
            "another tool's hook was dropped"
        );
        assert!(pre.iter().any(entry_is_ours), "our own hook was not added");
        assert!(after["hooks"]["SomeEventWeDoNotTouch"].is_array());

        // And removing ours puts it back exactly as it was.
        let cleaned = without_ours(&after);
        assert_eq!(cleaned, existing);
    }

    #[test]
    fn gemini_entries_carry_matcher_and_name() {
        let after = merged(&gemini::SPEC, EntryShape::Gemini, &json!({}));
        let entry = &after["hooks"]["BeforeTool"][0];
        assert_eq!(entry["matcher"], "*");
        assert_eq!(entry["hooks"][0]["name"], "coucou");
        let command = entry["hooks"][0]["command"].as_str().unwrap();
        assert!(command.contains("coucou-hook.exe\" google BeforeTool"), "got: {command}");
        assert_eq!(entry["hooks"][0]["timeout"], 10_000); // milliseconds
    }

    #[test]
    fn codex_config_only_adds_the_flag() {
        let with = with_codex_hooks_enabled(
            "model = \"gpt-5.1\"\n[features]\nunified_exec = true\n",
        )
        .unwrap();
        assert!(with.contains("model = \"gpt-5.1\""));
        assert!(with.contains("unified_exec = true"));
        assert!(with.contains("hooks = true"));
        // An empty file gets a bare [features] table.
        assert!(with_codex_hooks_enabled("").unwrap().contains("hooks = true"));
        // Broken TOML refuses, it does not get overwritten.
        assert!(with_codex_hooks_enabled("[unclosed").is_err());
    }

    #[test]
    fn a_fingerprint_notices_any_change() {
        assert_eq!(fingerprint_bytes(b"{}"), fingerprint_bytes(b"{}"));
        assert_ne!(fingerprint_bytes(b"{}"), fingerprint_bytes(b"{ }"));
        assert_ne!(fingerprint_bytes(b""), fingerprint_bytes(b"{}"));
    }

    /// Everything filesystem-shaped lives in one test on purpose: it points
    /// USERPROFILE at a temp directory, and that is process-wide.
    #[test]
    fn writing_backs_up_preserves_and_refuses_a_changed_file() {
        let spec = &claude::SPEC;
        let tmp = std::env::temp_dir().join(format!("coucou-hooks-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(tmp.join(".claude")).unwrap();
        std::env::set_var("USERPROFILE", &tmp);

        let path = spec.hooks_path();
        assert!(path.starts_with(&tmp), "the test must not touch the real home");

        // A real-shaped file, written the way PowerShell 5 would: UTF-8 with BOM.
        let original = r#"{"model":"claude-opus-5","theme":"dark","tui":{"x":1},"hooks":{"PreToolUse":[{"hooks":[{"type":"command","command":"other-tool.exe"}]}]}}"#;
        let mut bytes = vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(original.as_bytes());
        std::fs::write(&path, &bytes).unwrap();

        // Install.
        let plan = spec.preview(true).expect("a BOM must not stop the preview");
        assert!(plan.diff.contains("coucou-hook"), "the diff must show what changes");
        let backup = spec.write(true, &plan.fingerprint).expect("install should succeed");

        // The backup holds the original bytes, BOM and all.
        assert_eq!(std::fs::read(&backup).unwrap(), bytes);

        // Everything else survived, and so did the other tool's hook.
        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after["model"], "claude-opus-5");
        assert_eq!(after["theme"], "dark");
        assert_eq!(after["tui"]["x"], 1);
        let pre = after["hooks"]["PreToolUse"].as_array().unwrap();
        assert!(pre.iter().any(|e| serde_json::to_string(e).unwrap().contains("other-tool.exe")));
        assert!(spec.status().installed);

        // A file that moved since the preview is refused, and left alone.
        let stale = spec.preview(false).unwrap();
        std::fs::write(&path, br#"{"model":"someone-else-edited-this"}"#).unwrap();
        let err = spec.write(false, &stale.fingerprint).unwrap_err();
        assert!(err.contains("changed since the preview"), "got: {err}");
        let untouched: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(untouched["model"], "someone-else-edited-this");

        // Content we cannot parse is refused before anything is written.
        std::fs::write(&path, b"{ broken").unwrap();
        assert!(spec.preview(true).is_err());
        assert!(spec.write(true, "whatever").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"{ broken");

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
