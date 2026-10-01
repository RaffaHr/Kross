# Coucou — guide for AI coding agents

Coucou is a native macOS app: Mochi, a small animated character living in the MacBook notch, shows Claude Code sessions and a few integrations, and lets the user approve, answer, chat and drop files from the notch.

## Where things are
- `NotchBuddy/Sources/App/` — all Swift code. `NotchBuddy/Resources/sounds/` — the 28 WAV sounds. `NotchBuddy/project.yml` — XcodeGen project (never edit the `.xcodeproj` by hand).
- `docs/SPEC.md`, `docs/INTEGRATIONS.md` — behaviour, views, states, integrations (in French).
- `design/prototype/notch-buddy.html` — original prototype, the visual source of truth. `design/captures/` — target screenshots.
- `docs/*.html` — the GitHub Pages site (privacy, terms, support, legal notice).

## Build
```
cd NotchBuddy && xcodegen && xcodebuild -scheme NotchBuddy -configuration Debug build
```

## Rules
- Swift 6, SwiftUI + AppKit. No third-party dependencies unless truly unavoidable. The character is drawn in code (`Canvas` + `TimelineView`), no Rive/Lottie/images.
- Secrets live in the Keychain, never on disk or in git.
- No telemetry. Network calls only to services the user configured.
- Never block Claude Code: if the app doesn't answer, the hook exits immediately.
- Never overwrite `~/.claude/settings.json`: dated backup, merge, show the diff, write only after the user confirms.
- Never send an email or approve a Claude Code permission without an explicit click.
- Performance: 0 % CPU when the island is hidden.
- Keep the bundle identifier `fr.louisraille.NotchBuddy` (Keychain items, preferences and permissions depend on it).
- Visual changes must match the prototype and the screenshots in `design/captures/`.

<!-- specdriven:begin -->
## SpecDriven workflow
- Canonical repository with local gates: `windows/` only (repo name `windows`, root `windows/`). The macOS app (`NotchBuddy/`) cannot be verified on this machine — no Xcode; its build proof is `.github/workflows/build.yml` on `macos-latest`. Changes under `NotchBuddy/` have no local gate: say so explicitly in the plan and rely on CI + review.
- A second repository entry `coucou` (project root) exists carrying no code: it scopes fingerprints and upstream artifacts (`.specdriven/`, `CONTEXT.md`, `docs/adr/`) which must resolve under a configured root. Readiness demands every fingerprinted repo show all canonical gates PASS, so its gates are integrity-only (`git rev-parse --verify HEAD`) — an honest assertion that the checkout is valid, not fake quality signal.
- Consumers: `["windows"]`. macOS↔Windows parity is OFF by setup decision — divergences are intentional and documented in `windows/README.md` ("What's different"). Re-evaluate only if a behaviour is declared shared.
- Git remotes: `origin` = `RaffaHr/Kross` (working fork), `upstream` = `Louis-CFM/coucou` (original). The fork has GitHub Issues disabled, so the tracker is **file-based** (`tracker.type: file`) — items live in `.specdriven/tickets/T-*.md` with `blockedBy` frontmatter. Upstream stages: triage **required** (every change gets a tracker disposition), spec **required** (`.specdriven/specs/SPEC-*.md` before planning), tickets optional, wayfind off.
- TDD mode `seams`: the planner marks TDD seams in the plan; `tdd-driver` writes the red test before `coder` implements there.
- Quality gates (`windows` root): lint = `cargo clippy --workspace --all-targets`; typecheck = `npx tsc --noEmit`; build = `npm run build` + `cargo check --workspace --all-targets`; unit = `cargo test --workspace`; coverage = `cargo tarpaulin --workspace --lib --out Lcov --output-dir target/specdriven-coverage --timeout 300`. Coverage runs real tarpaulin (installed 2026-10-01, ~23.5 % line coverage at SPEC-001 close) but declares **no `lcov` assertion** — the workflow's strict `coverageMinimum: 100` is unattainable for GUI/process code; the gate verifies the instrumented suite passes and produces the report, not 100 %. Recorded as a known softness alongside the lint/format state. Known softness: clippy has 3 warnings and `cargo fmt --check` shows diffs — tightening to `-D warnings` / `fmt` is a pending decision, not silently assumed.
- Contract gate: no commands configured — any `API_REQUIRED` change is `MISSING_CAPABILITY` until a contract test runner exists. Most changes here are `NO_API` (the Claude Code hook protocol is file/pipe-based, not an HTTP API surface).
- Surfaces: `web: true` — the island front end runs in a plain browser via `npm run dev` (incl. `dev/upload-preview.html`), so playwright-tester applies to UI changes. `mobile: false`, `e2e: false` (no e2e harness).
- Architecture style: `tauri-desktop` — `windows/src/` framework-free TypeScript front end (Canvas 2D Mochi, island state machine, views, settings), `windows/src-tauri/` Rust backend (window, named pipe, Claude API, pollers, keyring), `windows/hook/` `coucou-hook.exe` relay. Product rules live in the sections above plus `docs/SPEC.md`, `docs/INTEGRATIONS.md`, `windows/README.md`.
- Worktrees for parallel work live in `C:/Users/Raffa/Documents/code/coucou-worktrees` — only with explicit user approval.
- Developer profile: solo maintainer (Louis Raille); reviews value compact diffs, honest N/A over fake PASS, and the product rules above (never block Claude Code, Keychain/Credential Manager for secrets, 0 % CPU hidden, no telemetry).
<!-- specdriven:end -->
