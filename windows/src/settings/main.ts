// Settings window — the place where anything that writes to disk is confirmed.
// Stage 2 covers the Claude Code hooks and the general preferences; API keys and
// integrations land here too in a later stage.

import "./settings.css";
import {
  Bridge,
  onEvent,
  type HookStatus,
  type OauthComplete,
  type ProviderInfo,
} from "../core/bridge";
import { DEFAULT_SETTINGS, type Settings } from "../core/state";
import { h, clear } from "../views/dom";

let settings: Settings = { ...DEFAULT_SETTINGS };
let version = "";

const root = document.getElementById("settings-root")!;

async function save() {
  await Bridge.saveSettings(settings);
}

// ── Reusable bits ─────────────────────────────────────────────────────────────

function toggle(on: boolean, onChange: (v: boolean) => void): HTMLElement {
  const el = h("button", { class: on ? "switch on" : "switch", "aria-pressed": on });
  el.addEventListener("click", () => {
    const next = !el.classList.contains("on");
    el.classList.toggle("on", next);
    onChange(next);
  });
  return el;
}

function statusDot(ok: boolean): HTMLElement {
  return h("i", { class: "dot", style: `background:${ok ? "#22c55e" : "#f4505e"}` });
}

function renderDiff(text: string): HTMLElement {
  const box = h("div", { class: "diff" });
  for (const line of text.split("\n")) {
    const cls = line.startsWith("+") ? "add" : line.startsWith("-") ? "del" : "ctx";
    box.append(h("div", { class: cls, text: line }));
  }
  return box;
}

// ── Provider hooks panel ──────────────────────────────────────────────────────
// Every hookable provider gets this inside its card. It opens on its own once
// the provider is connected — the CLI's sessions are only worth watching when
// the provider is in use.

function hooksPanel(providerId: string): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column;gap:8px" });
  const wrap = h("div", {
    style: "display:flex;flex-direction:column;gap:8px;border-top:1px dashed rgba(255,255,255,.08);padding-top:8px",
  }, body);

  const draw = async () => {
    clear(body);
    body.append(h("div", { class: "hint", text: "Checking hook status…" }));
    let status: HookStatus | null = null;
    try {
      status = await Bridge.hooksStatus(providerId);
    } catch (err) {
      clear(body);
      body.append(h("div", { class: "notice err", text: String(err) }));
      return;
    }
    clear(body);
    if (!status) {
      body.append(h("div", { class: "hint", text: "Hook status unavailable." }));
      return;
    }
    const st = status;

    body.append(
      h("div", { class: "row" },
        statusDot(st.installed),
        h("span", { text: `${st.cli} hooks`, style: "font-weight:600" }),
      ),
      h("div", {
        class: "hint",
        text: st.installed
          ? `Coucou is hooked into your ${st.cli} sessions. Tool calls, questions and permission requests show up in the island.`
          : `Install the hooks to see your ${st.cli} sessions in the island and approve permissions without leaving what you are doing.`,
      }),
    );

    if (!st.cliDetected) {
      body.append(h("div", {
        class: "notice warn",
        text: `${st.cli} doesn't seem to be installed on this machine — the hooks can be installed anyway, but nothing will run them until the CLI is.`,
      }));
    }
    if (st.note) {
      body.append(h("div", { class: "hint", text: st.note }));
    }
    if (!st.hookReady) {
      body.append(h("div", {
        class: "notice warn",
        text: "coucou-hook.exe is not in place yet. Restart Coucou; if it still fails, build it with `cargo build -p coucou-hook`.",
      }));
    }

    body.append(
      h("div", { class: "row" },
        h("label", { text: "Config" }),
        h("span", { class: "path", text: st.settingsPath }),
      ),
      h("div", { class: "hint", text: `Events: ${st.events.join(", ")}` }),
    );

    const actions = h("div", { class: "row" });
    const install = h("button", {
      class: "primary",
      text: st.installed ? "Reinstall hooks…" : "Install hooks…",
      onclick: () => void showPreview(true),
    });
    // Writing hook commands that point at a relay which isn't there would give
    // every CLI session a broken hook and nothing to show for it.
    if (!st.hookReady) {
      install.disabled = true;
      install.title = "The relay isn't installed yet.";
    }
    actions.append(install);
    if (st.installed) {
      actions.append(h("button", {
        class: "danger",
        text: "Uninstall hooks…",
        onclick: () => void showPreview(false),
      }));
    }
    body.append(actions);
  };

  async function showPreview(install: boolean) {
    let preview;
    try {
      preview = await Bridge.hooksPreview(providerId, install);
    } catch (err) {
      // An unreadable or invalid config stops here rather than being treated
      // as empty and written over.
      clear(body);
      body.append(
        h("div", { class: "notice err", text: String(err).replace(/^Error:\s*/, "") }),
        h("div", { class: "row" }, h("button", {
          text: "Back",
          onclick: () => void draw(),
        })),
      );
      return;
    }
    if (!preview) return;
    clear(body);
    body.append(
      h("div", {
        class: "hint",
        text: install
          ? "This is exactly what will change in the CLI's config. Your own hooks are left untouched."
          : "This removes Coucou's entries only. Your own hooks are left untouched.",
      }),
      renderDiff(preview.diff),
      h("div", { class: "row" },
        h("span", { class: "path", text: `Backup → ${preview.backup}` }),
      ),
    );
    const confirm = h("button", {
      class: install ? "primary" : "danger",
      text: install ? "Back up and write" : "Back up and remove",
    });
    confirm.addEventListener("click", async () => {
      confirm.disabled = true;
      try {
        const backup = await Bridge.hooksApply(providerId, install, preview.fingerprint);
        clear(body);
        body.append(h("div", {
          class: "notice ok",
          text: `Done. Previous config saved as ${backup}. Open a new session to pick the hooks up.`,
        }));
        window.setTimeout(() => void draw(), 2600);
      } catch (err) {
        confirm.disabled = false;
        body.append(h("div", { class: "notice err", text: `Could not write: ${String(err)}` }));
      }
    });
    body.append(h("div", { class: "row" }, confirm, h("button", {
      text: "Cancel",
      onclick: () => void draw(),
    })));
  }

  void draw();
  return wrap;
}

// ── AI providers section ──────────────────────────────────────────────────────
//
// Each provider gets a card: active radio, connection status, an API-key field
// and — where the provider allows it — a "Sign in" OAuth button. The keys and
// tokens live in the Credential Manager; the window only ever sees "connected".

function providerCard(p: ProviderInfo, rebuild: () => void): HTMLElement {
  const card = h("div", {
    style: "display:flex;flex-direction:column;gap:8px;padding:10px 0;border-top:1px solid rgba(255,255,255,.08)",
  });
  const feedback = h("div", {});
  const note = (cls: string, text: string) => {
    clear(feedback);
    feedback.append(h("div", { class: `notice ${cls}`, text }));
  };

  // Header: active radio + name + status. The status text starts at "checking…"
  // when a credential exists — the probe below decides what it really is.
  const dot = statusDot(p.connected);
  const statusText = h("span", {
    class: "hint",
    text: p.connected ? "checking…" : "not connected",
  });
  const radio = h("button", {
    class: p.active ? "switch on" : "switch",
    "aria-pressed": p.active,
    title: p.active ? "Active provider" : "Make active",
  });
  radio.addEventListener("click", async () => {
    if (p.active) return;
    try {
      await Bridge.providerSetActive(p.id);
      rebuild();
    } catch (err) {
      note("err", String(err));
    }
  });
  card.append(
    h("div", { class: "row" },
      radio,
      h("span", { text: p.name, style: "font-weight:600" }),
      dot,
      statusText,
      p.active ? h("span", { class: "hint", text: "· answering the chat" }) : h("span", {}),
    ),
  );

  // OAuth sign-in (Claude/Codex/Google). The subscription path is the one the
  // user accepted in ADR-0002 — the warning stays next to the button.
  if (p.oauth) {
    const signIn = h("button", { text: `Sign in with ${p.name}` });
    if (!p.oauthConfigured) signIn.disabled = true;

    // Google can't ship its public client registration without tripping
    // secret scanners, so the user pastes the pair here (or exports
    // COUCOU_GOOGLE_CLIENT_ID / COUCOU_GOOGLE_CLIENT_SECRET).
    if (p.oauthClientFields) {
      const field = (label: string, key: "googleClientId" | "googleClientSecret", password: boolean) => {
        const input = h("input", {
          type: password ? "password" : "text",
          placeholder: `${label}…`,
          value: settings[key],
          autocomplete: "off",
          spellcheck: "false",
          style: "flex:1 1 auto;min-width:0",
        }) as HTMLInputElement;
        input.addEventListener("change", () => {
          settings[key] = input.value.trim();
          void save().then(rebuild);
        });
        return h("div", { class: "row" }, h("label", { text: label }), input);
      };
      card.append(
        field("OAuth client ID", "googleClientId", false),
        field("OAuth client secret", "googleClientSecret", true),
        h("div", {
          class: "hint",
          text: "Google sign-in needs the public OAuth client pair — e.g. the values gemini-cli ships — or set COUCOU_GOOGLE_CLIENT_ID / COUCOU_GOOGLE_CLIENT_SECRET.",
        }),
      );
    }

    const pasteRow = h("div", { class: "row", style: "display:none" });
    const paste = h("input", {
      type: "text",
      placeholder: "Paste the code the page shows",
      autocomplete: "off",
      spellcheck: "false",
      style: "flex:1 1 auto;min-width:0",
    }) as HTMLInputElement;
    const finish = h("button", { class: "primary", text: "Finish sign-in" });
    finish.addEventListener("click", async () => {
      const value = paste.value.trim();
      if (!value) return;
      finish.disabled = true;
      try {
        await Bridge.providerOauthFinish(p.id, value);
        note("ok", "Signed in.");
        rebuild();
      } catch (err) {
        finish.disabled = false;
        note("err", String(err));
      }
    });
    pasteRow.append(paste, finish);
    signIn.addEventListener("click", async () => {
      signIn.disabled = true;
      try {
        const begin = await Bridge.providerOauthBegin(p.id);
        if (begin) {
          Bridge.openUrl(begin.url);
          if (begin.expectsPaste) {
            pasteRow.style.display = "flex";
            paste.focus();
          } else {
            note("", "Waiting for the browser — finish signing in there.");
          }
        }
      } catch (err) {
        note("err", String(err));
        signIn.disabled = false;
      }
    });
    card.append(
      h("div", { class: "row" }, signIn),
      h("div", {
        class: "hint",
        text: "Sign-in uses your subscription the same way the provider's own CLI does — it may conflict with the provider's terms of service for third-party clients.",
      }),
      pasteRow,
    );
  }

  // API key row — always present; OAuth + key can coexist (OAuth wins).
  const keyField = h("input", {
    type: "password",
    placeholder: p.connected ? "••••••••  (stored)" : p.keyPlaceholder,
    autocomplete: "off",
    spellcheck: "false",
    style: "flex:1 1 auto;min-width:0",
  }) as HTMLInputElement;
  const saveKey = h("button", { text: "Save key" });
  saveKey.addEventListener("click", async () => {
    const value = keyField.value.trim();
    if (!value) return;
    try {
      await Bridge.secretSet(`provider-${p.id}-key`, value);
      keyField.value = "";
      note("ok", "Key saved. It never touches disk.");
      rebuild();
    } catch (err) {
      note("err", String(err));
    }
  });
  card.append(h("div", { class: "row" }, h("label", { text: "API key" }), keyField, saveKey));

  // Custom provider: base URL is a normal setting, not a secret.
  if (p.custom) {
    const base = h("input", {
      type: "text",
      placeholder: "https://your-endpoint/v1",
      value: settings.customBaseUrl,
      autocomplete: "off",
      spellcheck: "false",
      style: "flex:1 1 auto;min-width:0",
    }) as HTMLInputElement;
    base.addEventListener("change", () => {
      settings.customBaseUrl = base.value.trim();
      void save();
    });
    card.append(h("div", { class: "row" }, h("label", { text: "Base URL" }), base));
  }

  // Model: dropdown fed by the provider's own /models endpoint (the probe
  // below) with the bundled list as fallback; free text for custom.
  let modelSelect: HTMLSelectElement | null = null;
  if (p.models.length > 0 || !p.custom) {
    const model = h("select", {}) as HTMLSelectElement;
    modelSelect = model;
    const fill = (ids: string[]) => {
      clear(model);
      for (const id of ids) model.append(h("option", { value: id, text: id }));
      if (!ids.includes(p.model) && p.model) {
        model.append(h("option", { value: p.model, text: p.model }));
      }
      model.value = ids.includes(p.model) ? p.model : ids[0] ?? p.model;
    };
    fill(p.models);
    model.addEventListener("change", () => {
      settings.providerModels = { ...settings.providerModels, [p.id]: model.value };
      if (p.id === "claude") settings.model = model.value; // legacy field stays in sync
      void save();
    });
    card.append(h("div", { class: "row" }, h("label", { text: "Model" }), model));
  } else {
    const model = h("input", {
      type: "text",
      placeholder: "model name",
      value: p.model,
      autocomplete: "off",
      spellcheck: "false",
      style: "flex:1 1 auto;min-width:0",
    }) as HTMLInputElement;
    model.addEventListener("change", () => {
      settings.providerModels = { ...settings.providerModels, [p.id]: model.value.trim() };
      void save();
    });
    card.append(h("div", { class: "row" }, h("label", { text: "Model" }), model));
  }

  // The real connection check: one API call against the models endpoint. Its
  // answer sets the status text and refreshes the dropdown with live models.
  if (p.connected) {
    void Bridge.providerProbe(p.id).then((result) => {
      if (!result) return;
      switch (result.state) {
        case "connected":
          statusText.textContent = "connected";
          dot.style.background = "#22c55e";
          if (modelSelect && result.models.length > 0) {
            p.models = result.models;
            const current = modelSelect.value;
            const fillEvent = result.models.includes(current);
            clear(modelSelect);
            for (const id of result.models) {
              modelSelect.append(h("option", { value: id, text: id }));
            }
            if (!fillEvent && p.model && !result.models.includes(p.model)) {
              modelSelect.append(h("option", { value: p.model, text: p.model }));
            }
            modelSelect.value = result.models.includes(current) ? current
              : (p.model && result.models.includes(p.model)) ? p.model : result.models[0];
          }
          break;
        case "failed":
          statusText.textContent = "credential rejected";
          dot.style.background = "#f4505e";
          if (result.error) note("err", `Connection failed: ${result.error}`);
          break;
        case "unverified":
          statusText.textContent = "credential stored (unverified)";
          dot.style.background = "#f5a524";
          break;
        default:
          statusText.textContent = "not connected";
          dot.style.background = "#f4505e";
          if (result.error) note("err", result.error);
      }
    });
  }

  // Per-provider hooks: each CLI's own config file and event set, merged
  // without touching anything else. The panel lives inside the card because
  // the hooks only matter while the provider is connected.
  if (p.hooksSupported) {
    card.append(hooksPanel(p.id));
  }

  if (p.connected) {
    const disconnect = h("button", { class: "danger", text: "Disconnect" });
    disconnect.addEventListener("click", async () => {
      try {
        await Bridge.providerDisconnect(p.id);
        note("ok", "Signed out and key removed.");
        rebuild();
      } catch (err) {
        note("err", String(err));
      }
    });
    card.append(h("div", { class: "row" }, disconnect));
  }

  card.append(feedback);
  return card;
}

function providersSection(list: ProviderInfo[]): HTMLElement {
  const body = h("div", { style: "display:flex;flex-direction:column" });

  const rebuild = () => {
    void Bridge.providersList().then((fresh) => {
      clear(body);
      for (const p of fresh ?? list) body.append(providerCard(p, rebuild));
    });
  };
  for (const p of list) body.append(providerCard(p, rebuild));

  // Loopback sign-ins land here when the browser comes back.
  void onEvent<OauthComplete>("provider-oauth-complete", (done) => {
    rebuild();
    if (!done.ok && done.error) {
      body.prepend(h("div", { class: "notice err", text: done.error }));
    }
  });

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: "Providers" })),
    h("div", {
      class: "hint",
      text: "The provider you activate answers every chat. Keys and sign-ins live in the Windows Credential Manager, never on disk.",
    }),
    body,
  );
}

// ── Integrations section ──────────────────────────────────────────────────────

interface IntegrationDef {
  id: string;
  name: string;
  color: string;
  /** Credential Manager keys, in the order they are shown. */
  fields: { key: string; label: string; placeholder: string; secret: boolean }[];
}

const INTEGRATIONS: IntegrationDef[] = [
  { id: "integration_stripe", name: "Stripe", color: "#0570DE",
    fields: [{ key: "stripe-api-key", label: "Secret key", placeholder: "sk_live_…", secret: true }] },
  { id: "integration_github", name: "GitHub", color: "#F4505E",
    fields: [{ key: "github-token", label: "Token", placeholder: "ghp_…", secret: true }] },
  { id: "integration_vercel", name: "Vercel", color: "#7C5CFF",
    fields: [{ key: "vercel-token", label: "Token", placeholder: "…", secret: true }] },
  { id: "integration_n8n", name: "n8n", color: "#F29B38",
    fields: [
      { key: "n8n-url", label: "Instance URL", placeholder: "https://n8n.example.com", secret: false },
      { key: "n8n-api-key", label: "API key", placeholder: "…", secret: true },
    ] },
  { id: "integration_resend", name: "Resend", color: "#22C55E",
    fields: [{ key: "resend-api-key", label: "API key", placeholder: "re_…", secret: true }] },
  { id: "integration_notion", name: "Notion", color: "#8C8C8C",
    fields: [{ key: "notion-api-key", label: "Integration token", placeholder: "ntn_…", secret: true }] },
  { id: "integration_calcom", name: "Cal.com", color: "#C9956A",
    fields: [{ key: "calcom-api-key", label: "API key", placeholder: "cal_…", secret: true }] },
];

const MAX_ACTIVE = 4;

function integrationsSection(present: Record<string, boolean>): HTMLElement {
  const note = h("div", { class: "hint" });
  const list = h("div", { style: "display:flex;flex-direction:column;gap:14px" });

  function updateNote() {
    const used = settings.activeIntegrations.length;
    note.textContent = `Pick up to ${MAX_ACTIVE} pills to show next to Mochi — ${used}/${MAX_ACTIVE} in use. Keys are stored in the Windows Credential Manager, never on disk.`;
  }

  for (const def of INTEGRATIONS) {
    const active = settings.activeIntegrations.includes(def.id);
    const sw = h("button", { class: active ? "switch on" : "switch" });
    sw.addEventListener("click", () => {
      const on = settings.activeIntegrations.includes(def.id);
      if (on) {
        settings.activeIntegrations = settings.activeIntegrations.filter((x) => x !== def.id);
      } else {
        if (settings.activeIntegrations.length >= MAX_ACTIVE) return;
        settings.activeIntegrations = [...settings.activeIntegrations, def.id];
      }
      sw.classList.toggle("on", !on);
      updateNote();
      void save();
    });

    const rows = h("div", { style: "display:flex;flex-direction:column;gap:6px;flex:1 1 auto;min-width:0" });
    for (const field of def.fields) {
      const input = h("input", {
        type: field.secret ? "password" : "text",
        placeholder: present[field.key] ? "••••••••  (stored)" : field.placeholder,
        autocomplete: "off",
        spellcheck: "false",
        style: "flex:1 1 auto;min-width:0",
      }) as HTMLInputElement;
      const saveBtn = h("button", { text: "Save" });
      const dotEl = statusDot(present[field.key] ?? false);
      saveBtn.addEventListener("click", async () => {
        const value = input.value.trim();
        try {
          await Bridge.secretSet(field.key, value);
          present[field.key] = value.length > 0;
          input.value = "";
          input.placeholder = value ? "••••••••  (stored)" : field.placeholder;
          dotEl.style.background = value ? "#22c55e" : "#f4505e";
        } catch {
          dotEl.style.background = "#f5a524";
        }
      });
      rows.append(
        h("div", { class: "row" },
          h("label", { style: "min-width:104px", text: field.label }),
          input, saveBtn, dotEl,
        ),
      );
    }

    list.append(
      h("div", { style: "display:flex;gap:12px;align-items:flex-start" },
        h("div", { style: "display:flex;align-items:center;gap:8px;min-width:132px;padding-top:4px" },
          sw,
          h("i", { class: "dot", style: `background:${def.color}` }),
          h("span", { style: "font-size:12.5px", text: def.name }),
        ),
        rows,
      ),
    );
  }

  updateNote();
  return h("section", {}, h("h2", {}, h("span", { text: "Integrations" })), note, list);
}

// ── General section ───────────────────────────────────────────────────────────

function generalSection(): HTMLElement {
  const volume = h("input", {
    type: "range", min: "0", max: "0.2", step: "0.005",
    value: String(settings.soundVolume),
  }) as HTMLInputElement;
  volume.addEventListener("input", () => {
    settings.soundVolume = Number(volume.value);
    void save();
  });

  const autoClose = h("input", {
    type: "number", min: "5", max: "120", step: "1",
    value: String(Math.round(settings.autoCloseInterval)),
    style: "width:72px",
  }) as HTMLInputElement;
  autoClose.addEventListener("change", () => {
    settings.autoCloseInterval = Math.max(5, Math.min(120, Number(autoClose.value) || 15));
    autoClose.value = String(settings.autoCloseInterval);
    void save();
  });

  const screen = h("select", {}) as HTMLSelectElement;
  screen.append(
    h("option", { value: "primary", text: "Main display" }),
    h("option", { value: "cursor", text: "Display under the cursor" }),
  );
  screen.value = settings.screen;
  screen.addEventListener("change", () => {
    settings.screen = screen.value as Settings["screen"];
    void save();
  });

  return h(
    "section",
    {},
    h("h2", {}, h("span", { text: "General" })),
    h("div", { class: "row" },
      h("label", { text: "Sound" }),
      toggle(settings.soundEnabled, (v) => { settings.soundEnabled = v; void save(); }),
      volume,
    ),
    h("div", { class: "row" },
      h("label", { text: "Auto-close" }),
      autoClose,
      h("span", { class: "hint", text: "seconds after you leave the island" }),
    ),
    h("div", { class: "row" },
      h("label", { text: "Island lives on" }),
      screen,
    ),
    h("div", { class: "row" },
      h("label", { text: "Launch at startup" }),
      toggle(settings.autostart, (v) => { settings.autostart = v; void save(); }),
    ),
  );
}

// ── Boot ──────────────────────────────────────────────────────────────────────

async function main() {
  const boot = await Bridge.boot();
  if (boot) {
    settings = { ...settings, ...boot.settings };
    version = boot.version;
  }
  const providers = (await Bridge.providersList()) ?? [];

  const keys = [
    "stripe-api-key", "github-token", "vercel-token",
    "n8n-url", "n8n-api-key", "resend-api-key", "notion-api-key", "calcom-api-key",
  ];
  const present: Record<string, boolean> = {};
  for (const k of keys) present[k] = (await Bridge.secretPresent(k)) ?? false;

  clear(root);
  root.append(
    h("h1", {}, h("span", { text: "Coucou" }), h("span", { class: "version", text: version })),
    providersSection(providers),
    integrationsSection(present),
    generalSection(),
    h("div", {
      class: "hint",
      text: "No telemetry. Network requests only go to the services you configure yourself.",
    }),
  );

  void onEvent<Settings>("settings-changed", (s) => {
    settings = { ...settings, ...s };
  });
}

void main();
