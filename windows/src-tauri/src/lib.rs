// Coucou for Windows — app wiring and the commands the island calls.

mod files;
mod hooks;
mod integrations;
mod island;
mod log;
mod pipe;
mod providers;
mod secrets;
mod settings;
mod tray;
mod win_user;

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{ManagerExt, MacosLauncher};

use files::DroppedFile;
use hooks::{HookPreview, HookStatus};
use island::{PollGate, ScreenInfo};
use pipe::Pending;
use providers::{Chat, ChatContext, ChatReply};
use settings::Settings;

/// Keeps spawned helpers from flashing a console window.
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub struct Shared {
    pub settings: Mutex<Settings>,
    pub gate: Arc<PollGate>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BootInfo {
    settings: Settings,
    screen: ScreenInfo,
    version: String,
    hook_path: String,
}

#[tauri::command]
fn boot(app: AppHandle, shared: State<Shared>) -> BootInfo {
    let mut settings = shared.settings.lock().unwrap().clone();
    // The real state of ~/.claude/settings.json wins over whatever we stored.
    settings.hooks_installed = hooks::spec_for("claude")
        .map(|spec| spec.status().installed)
        .unwrap_or(false);
    let screen = island::screen_info(&app, &settings.screen);
    BootInfo {
        settings,
        screen,
        version: env!("CARGO_PKG_VERSION").to_string(),
        hook_path: settings::hook_exe_path().to_string_lossy().to_string(),
    }
}

#[tauri::command]
fn save_settings(app: AppHandle, shared: State<Shared>, settings: Settings) {
    let (screen_changed, autostart_changed) = {
        let mut current = shared.settings.lock().unwrap();
        let screen_changed = current.screen != settings.screen;
        let autostart_changed = current.autostart != settings.autostart;
        *current = settings.clone();
        (screen_changed, autostart_changed)
    };
    if let Err(err) = settings::save(&settings) {
        eprintln!("[coucou] could not save settings: {err}");
    }
    if autostart_changed {
        let manager = app.autolaunch();
        let result = if settings.autostart { manager.enable() } else { manager.disable() };
        if let Err(err) = result {
            eprintln!("[coucou] autostart: {err}");
        }
    }
    if screen_changed {
        let collapsed = shared.gate.collapsed.load(Ordering::Relaxed);
        island::apply_geometry(&app, &settings.screen, collapsed);
    }
    // Keep the other window in step (island ⇄ settings window).
    let _ = app.emit("settings-changed", settings);
}

/// Hidden island → shrink the window to the invisible wake strip and park the
/// cursor poll; anything else → full panel and 60 Hz polling.
#[tauri::command]
fn set_collapsed(app: AppHandle, shared: State<Shared>, collapsed: bool) {
    let pref = shared.settings.lock().unwrap().screen.clone();
    shared.gate.collapsed.store(collapsed, Ordering::Relaxed);
    island::apply_geometry(&app, &pref, collapsed);
    // The wake strip must always take the mouse, and a resize invalidates the flag.
    island::set_ignore_cursor(&app, false);
    shared.gate.forget_ignore_state();
    shared.gate.set_active(!collapsed);
}

/// The front end pushes the island shape; Rust decides click-through from it.
#[tauri::command]
fn set_island_rect(shared: State<Shared>, x: f64, y: f64, width: f64, height: f64) {
    shared.gate.set_rect(island::IslandRect { x, y, w: width, h: height });
}

#[tauri::command]
fn focus_window(app: AppHandle, focused: bool) {
    let Some(win) = island::window(&app) else { return };
    island::set_activating(&win, focused);
    if focused {
        let _ = win.set_focus();
    }
}

#[tauri::command]
fn reposition(app: AppHandle, shared: State<Shared>) {
    let pref = shared.settings.lock().unwrap().screen.clone();
    let collapsed = shared.gate.collapsed.load(Ordering::Relaxed);
    island::apply_geometry(&app, &pref, collapsed);
}

#[tauri::command]
fn open_url(url: String) {
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return;
    }
    let _ = Command::new("rundll32.exe")
        .args(["url.dll,FileProtocolHandler", &url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// "Open terminal" opens the working folder in VS Code when `code` is on PATH,
/// and falls back to Explorer otherwise.
#[tauri::command]
fn open_in_vscode(path: Option<String>) -> bool {
    // No `cmd /C` anywhere near this. The path is a project folder chosen by
    // whoever is using Claude Code, and cmd would happily read `&`, `^` and `%`
    // in a folder name as syntax. Finding the launcher ourselves and handing the
    // path over as a separate argument keeps it a path.
    if let Some(code) = find_on_path("code") {
        let mut cmd = Command::new(code);
        if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
            cmd.arg(p);
        }
        if cmd.creation_flags(CREATE_NO_WINDOW).spawn().is_ok() {
            return true;
        }
    }
    if let Some(p) = path.as_deref().filter(|p| !p.is_empty()) {
        let _ = Command::new("explorer").arg(p).spawn();
    }
    false
}

/// Our own `where`: walks %PATH% against %PATHEXT%, no shell involved.
/// Rust quotes arguments correctly for `.cmd`/`.bat` targets since 1.77, so
/// spawning `code.cmd` directly is safe.
fn find_on_path(stem: &str) -> Option<std::path::PathBuf> {
    let exts = std::env::var("PATHEXT").unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into());
    let dirs = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&dirs) {
        for ext in exts.split(';').filter(|e| !e.is_empty()) {
            let candidate = dir.join(format!("{stem}{}", ext.to_lowercase()));
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

#[tauri::command]
fn quit_app(app: AppHandle) {
    app.exit(0);
}

/// Tray → Pause. Paused means paused: the pollers stop talking to the network,
/// not just the island stopping showing things.
#[tauri::command]
fn set_paused(paused: bool) {
    integrations::set_paused(paused);
}

// ── Provider CLI hooks ────────────────────────────────────────────────────────

/// The CLI spec a provider id maps to; providers without a hookable CLI
/// (hermes, custom) simply error out — the UI hides their panel anyway.
fn hooks_spec(provider: &str) -> Result<&'static hooks::CliSpec, String> {
    hooks::spec_for(provider)
        .ok_or_else(|| format!("Provider '{provider}' has no hookable CLI."))
}

#[tauri::command]
fn hooks_status(provider: String) -> Result<HookStatus, String> {
    Ok(hooks_spec(&provider)?.status())
}

/// Returns the diff the user has to look at before anything is written.
#[tauri::command]
fn hooks_preview(provider: String, install: bool) -> Result<HookPreview, String> {
    hooks_spec(&provider)?.preview(install)
}

/// Only ever called from an explicit click in the settings window.
#[tauri::command]
fn hooks_apply(
    app: AppHandle,
    shared: State<Shared>,
    provider: String,
    install: bool,
    fingerprint: String,
) -> Result<String, String> {
    // The fingerprint comes from the preview the user actually looked at, so a
    // config that changed in between is refused rather than overwritten.
    let backup = hooks_spec(&provider)?.write(install, &fingerprint)?;
    if provider == "claude" {
        // `settings.hooks_installed` is the island's "any hooks?" hint and has
        // always meant Claude's file; keep it truthful.
        let updated = {
            let mut current = shared.settings.lock().unwrap();
            current.hooks_installed = install;
            let _ = settings::save(&current);
            current.clone()
        };
        let _ = app.emit("settings-changed", updated);
    }
    Ok(backup)
}

#[tauri::command]
fn approval_decision(app: AppHandle, request_id: String, decision: String) {
    pipe::answer(&app, &request_id, &decision);
}

/// The island has the card on screen, so the long wait for a human may begin.
/// Until this arrives the relay only waits a few hundred milliseconds, which is
/// what stops a paused or unresponsive island from freezing Claude Code.
#[tauri::command]
fn approval_ack(app: AppHandle, request_id: String) {
    pipe::acknowledge(&app, &request_id);
}

/// Nobody can act on this request — the island is paused, or another card is
/// already up. Claude Code falls back to asking in the terminal immediately.
#[tauri::command]
fn approval_decline(app: AppHandle, request_id: String) {
    pipe::decline(&app, &request_id);
}

// ── Chat, files and secrets ───────────────────────────────────────────────────

/// One chat turn, routed through the active provider. The credential and any
/// file bytes stay on the Rust side.
#[tauri::command]
async fn chat_send(
    shared: State<'_, Shared>,
    chat: State<'_, Chat>,
    query: String,
    context: Option<ChatContext>,
) -> Result<ChatReply, String> {
    let settings = shared.settings.lock().unwrap().clone();
    providers::send(&chat, &settings, query, context).await
}

#[tauri::command]
fn chat_reset(chat: State<Chat>) {
    chat.reset();
}

// ── Providers ────────────────────────────────────────────────────────────────

/// A sign-in in progress, kept between `provider_oauth_begin` (which returns
/// the URL + whether the user must paste a code) and `provider_oauth_finish`.
#[derive(Default)]
pub struct OauthInFlight(Mutex<std::collections::HashMap<String, providers::oauth::Pending>>);

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    id: String,
    name: String,
    /// "Sign in with …" exists for this provider.
    oauth: bool,
    /// The client registration is user-supplied — show id/secret inputs
    /// (Google, whose public pair can't ship without tripping scanners).
    oauth_client_fields: bool,
    /// A sign-in can actually start: static client, or configured via
    /// Settings/env. Otherwise the button stays disabled.
    oauth_configured: bool,
    connected: bool,
    active: bool,
    model: String,
    models: Vec<String>,
    key_placeholder: String,
    /// True for the OpenAI-compatible custom provider — needs a base URL field.
    custom: bool,
    /// The provider's CLI has a hook surface we can install into.
    hooks_supported: bool,
    /// That CLI's display name ("Claude Code", "Codex CLI", "Gemini CLI").
    cli_name: Option<String>,
    /// Extra instruction shown in the hooks panel (e.g. Codex's trust review).
    hooks_note: Option<String>,
}

#[tauri::command]
fn providers_list(shared: State<Shared>) -> Vec<ProviderInfo> {
    let settings = shared.settings.lock().unwrap().clone();
    providers::PROVIDERS
        .iter()
        .map(|spec| ProviderInfo {
            id: spec.id.to_string(),
            name: spec.name.to_string(),
            oauth: spec.oauth.is_some(),
            oauth_client_fields: spec
                .oauth
                .as_ref()
                .map(|o| o.client_id.is_empty())
                .unwrap_or(false),
            oauth_configured: spec
                .oauth
                .as_ref()
                .map(|_| providers::oauth::client_credentials(spec, &settings).is_ok())
                .unwrap_or(false),
            connected: providers::connected(spec),
            active: settings.active_provider == spec.id,
            model: providers::model_for(spec, &settings),
            models: spec.models.iter().map(|m| m.to_string()).collect(),
            key_placeholder: spec.key_placeholder.to_string(),
            custom: spec.id == "custom",
            hooks_supported: hooks::spec_for(spec.id).is_some(),
            cli_name: hooks::spec_for(spec.id).map(|s| s.cli.to_string()),
            hooks_note: hooks::spec_for(spec.id).and_then(|s| s.note.map(str::to_string)),
        })
        .collect()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OauthBegin {
    url: String,
    /// Claude's public client redirects to a hosted "paste this code" page.
    expects_paste: bool,
}

/// "Sign in with …": opens the provider's consent page. Loopback providers
/// finish in the background and emit `provider-oauth-complete`; paste providers
/// wait for `provider_oauth_finish`.
#[tauri::command]
fn provider_oauth_begin(
    app: AppHandle,
    shared: State<Shared>,
    in_flight: State<OauthInFlight>,
    id: String,
) -> Result<OauthBegin, String> {
    let spec = providers::spec_of(&id);
    let settings = shared.settings.lock().unwrap().clone();
    let started = providers::oauth::begin(spec, &settings)?;
    in_flight.0.lock().unwrap().insert(spec.id.to_string(), started.pending);
    if let Some(rx) = started.receiver {
        // The loopback listener is blocking; park it on its own task and
        // report the outcome as an event the settings window can show.
        tauri::async_runtime::spawn(async move {
            let code = match tokio::task::spawn_blocking(move || rx.recv()).await {
                Ok(Ok(result)) => result,
                _ => Err("Sign-in listener stopped unexpectedly.".into()),
            };
            let outcome: Result<(), String> = match code {
                Ok(code) => {
                    let pending = {
                        let state = app.state::<OauthInFlight>();
                        let removed = state.0.lock().unwrap().remove(spec.id);
                        removed
                    };
                    match pending {
                        Some(pending) => providers::oauth::exchange(spec, &pending, &code, &settings)
                            .await
                            .map(|_| ()),
                        None => Err("Sign-in expired — start over.".into()),
                    }
                }
                Err(err) => Err(err),
            };
            let _ = app.emit(
                "provider-oauth-complete",
                serde_json::json!({ "id": spec.id, "ok": outcome.is_ok(), "error": outcome.err() }),
            );
        });
    }
    Ok(OauthBegin { url: started.begin.url, expects_paste: started.begin.expects_paste })
}

/// Claude's flow: the user pastes the `code#state` the hosted page showed.
#[tauri::command]
async fn provider_oauth_finish(
    shared: State<'_, Shared>,
    in_flight: State<'_, OauthInFlight>,
    id: String,
    pasted: String,
) -> Result<(), String> {
    let spec = providers::spec_of(&id);
    let pending = in_flight.0.lock().unwrap().remove(spec.id).ok_or_else(|| {
        "No sign-in is in progress — click Sign in first.".to_string()
    })?;
    let settings = shared.settings.lock().unwrap().clone();
    providers::oauth::finish_paste(spec, pending, &pasted, &settings).await.map(|_| ())
}

/// "Is this credential real?" — resolves it, then makes one real API call
/// (the models listing doubles as the probe) and reports what happened.
#[tauri::command]
async fn provider_probe(shared: State<'_, Shared>, id: String) -> Result<providers::ProbeResult, String> {
    let spec = providers::spec_of(&id);
    let settings = shared.settings.lock().unwrap().clone();
    Ok(providers::probe(spec, &settings).await)
}

/// Sign out / remove every credential kind for a provider.
#[tauri::command]
fn provider_disconnect(app: AppHandle, id: String) -> Result<(), String> {
    if providers::spec_of(&id).id != id {
        return Err(format!("Unknown provider {id}"));
    }
    providers::disconnect(&id);
    let _ = app.emit("provider-oauth-complete", serde_json::json!({ "id": id, "ok": true, "error": null }));
    Ok(())
}

/// Switch the active provider. Chat history is reset — an assistant turn from
/// another provider may hold native blocks the new one can't read.
#[tauri::command]
fn provider_set_active(
    app: AppHandle,
    shared: State<Shared>,
    chat: State<Chat>,
    id: String,
) -> Result<(), String> {
    let spec = providers::spec_of(&id);
    let updated = {
        let mut current = shared.settings.lock().unwrap();
        current.active_provider = spec.id.to_string();
        let _ = settings::save(&current);
        current.clone()
    };
    chat.reset();
    let _ = app.emit("settings-changed", updated);
    Ok(())
}

/// Copies a dropped file into the inbox and reports its name back.
#[tauri::command]
fn ingest_file(path: String) -> Result<DroppedFile, String> {
    files::ingest(&path)
}

/// The island may only ask whether a key exists — never read it.
#[tauri::command]
fn secret_present(key: String) -> bool {
    secrets::present(&key)
}

#[tauri::command]
fn secret_set(key: String, value: String) -> Result<(), String> {
    secrets::set(&key, &value)
}

#[tauri::command]
fn secret_clear(key: String) -> Result<(), String> {
    secrets::clear(&key)
}

/// Opens the configured n8n instance — the URL lives in the Credential Manager.
#[tauri::command]
fn open_n8n() {
    if let Some(url) = secrets::get("n8n-url") {
        open_url(url);
    }
}

/// Refresh buttons in the integration cards.
#[tauri::command]
async fn refresh_integration(app: AppHandle, id: String) {
    integrations::poll_once(app, &id).await;
}

/// Lets the island write to the same log as the Rust side.
#[tauri::command]
fn log_line(message: String) {
    log::line(format!("ui  {message}"));
}

// ── Settings window ───────────────────────────────────────────────────────────

/// WebView2 allows exactly one browser environment per app, and its options are
/// fixed by whichever webview is created first. Every window must therefore ask
/// for the *same* arguments as the island (see `additionalBrowserArgs` in
/// tauri.conf.json) — a mismatch makes the second window come up blank, with no
/// error anywhere.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection --autoplay-policy=no-user-gesture-required";

/// In a dev build the pages are served by Vite, so the second window needs the
/// absolute dev URL; a bundled build resolves it inside the app bundle.
fn settings_page_url(app: &AppHandle) -> WebviewUrl {
    #[cfg(dev)]
    if let Some(mut base) = app.config().build.dev_url.clone() {
        base.set_path("/settings.html");
        return WebviewUrl::External(base);
    }
    let _ = app;
    WebviewUrl::App("settings.html".into())
}

/// The settings window is created hidden at launch and only ever shown and
/// hidden afterwards. A WebView2 window created later — on the main thread or
/// not — silently comes up blank in this app, so the window that works is the
/// one that exists before the island's webview does.
fn create_settings_window(app: &AppHandle) {
    let url = settings_page_url(app);
    match WebviewWindowBuilder::new(app, "settings", url)
        .additional_browser_args(BROWSER_ARGS)
        .title("Settings — Coucou")
        .inner_size(560.0, 680.0)
        .min_inner_size(460.0, 480.0)
        .resizable(true)
        .visible(false)
        .center()
        .build()
    {
        Ok(win) => {
            // Closing it must only hide it, or it could never be reopened.
            let hidden = win.clone();
            win.on_window_event(move |event| {
                if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                    api.prevent_close();
                    let _ = hidden.hide();
                }
            });
        }
        Err(err) => log::line(format!("settings window failed: {err}")),
    }
}

pub fn show_settings_window(app: &AppHandle) {
    let Some(win) = app.get_webview_window("settings") else {
        log::line("settings window missing");
        return;
    };
    let _ = win.unminimize();
    let _ = win.show();
    let _ = win.set_focus();
}

#[tauri::command]
fn open_settings_window(app: AppHandle) {
    show_settings_window(&app);
}

pub fn run() {
    let loaded = settings::load();
    let gate = Arc::new(PollGate::new());

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            let _ = app.emit_to(island::WINDOW_LABEL, "tray", "open".to_string());
        }))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, None))
        .manage(Shared {
            settings: Mutex::new(loaded.clone()),
            gate: gate.clone(),
        })
        .manage(Pending::default())
        .manage(Chat::default())
        .manage(OauthInFlight::default())
        .invoke_handler(tauri::generate_handler![
            boot,
            save_settings,
            set_collapsed,
            set_island_rect,
            focus_window,
            reposition,
            open_url,
            open_in_vscode,
            quit_app,
            hooks_status,
            hooks_preview,
            hooks_apply,
            approval_decision,
            approval_ack,
            approval_decline,
            log_line,
            chat_send,
            chat_reset,
            providers_list,
            provider_set_active,
            provider_oauth_begin,
            provider_oauth_finish,
            provider_disconnect,
            provider_probe,
            ingest_file,
            secret_present,
            secret_set,
            secret_clear,
            refresh_integration,
            open_n8n,
            open_settings_window,
            set_paused,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            tray::build(&handle)?;
            // Before the island: see create_settings_window.
            create_settings_window(&handle);

            if let Some(win) = island::window(&handle) {
                island::make_non_activating(&win);
                island::apply_geometry(&handle, &loaded.screen, false);
                let _ = win.show();
            }
            gate.collapsed.store(false, Ordering::Relaxed);
            gate.set_active(true);
            island::spawn_cursor_poll(handle.clone(), gate.clone());

            log::line(format!("--- Coucou {} started ---", env!("CARGO_PKG_VERSION")));
            hooks::ensure_hook_exe(&handle);
            pipe::start(handle.clone());
            integrations::start(handle.clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Coucou");
}
