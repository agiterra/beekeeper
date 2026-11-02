// The Manage Host window. Presentation only: it holds no key, no model and no
// setup logic. Every read comes from the Host's inventory and every write is a
// named action that this app runs as its own `bee`. Text from the Host or the
// relay reaches the page through textContent, never as markup.

const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const byId = (id) => document.getElementById(id);

function el(tag, attrs, ...children) {
  const node = document.createElement(tag);
  for (const [key, value] of Object.entries(attrs || {})) {
    if (value === undefined || value === null) continue;
    if (key === "class") node.className = value;
    else if (key === "onclick") node.addEventListener("click", value);
    else if (key === "testid") node.dataset.testid = value;
    else node.setAttribute(key, value);
  }
  for (const child of children) {
    if (child === null || child === undefined) continue;
    node.append(child.nodeType ? child : String(child));
  }
  return node;
}

const short = (hex) =>
  hex && hex.length > 16 ? `${hex.slice(0, 8)}…${hex.slice(-6)}` : hex || "";

let inventory = null;
let signerOk = false;
let busy = false;

function keyFile() {
  return byId("key-file").value.trim() || null;
}

function spec() {
  const access = document.querySelector('input[name="access"]:checked').value;
  return {
    project: byId("project").value,
    channel: byId("channel").value,
    role: byId("role").value,
    displayName: byId("display-name").value.trim(),
    providerInstance: byId("runtime").value,
    model: byId("model").value,
    parallelism: 1,
    access:
      access === "owner"
        ? { mode: "owner_only" }
        : {
            mode: "allowlist",
            pubkeys: byId("allowlist").value.split(/\s+/).filter(Boolean),
          },
  };
}

function setBusy(next) {
  busy = next;
  updateControls();
}

// A write is offered only when the key has been shown to be the Host's owner,
// and only one runs at a time. Disabled with a reason, never hidden.
function updateControls() {
  const offered = (runtime()?.selectableModels || []).length > 0;
  const ready = signerOk && !busy;
  byId("set-up").disabled = !(
    ready &&
    offered &&
    byId("project").value &&
    byId("channel").value &&
    byId("role").value
  );
  byId("preflight").disabled = !(
    ready &&
    byId("project").value &&
    byId("channel").value &&
    byId("role").value
  );
  byId("trust").disabled = !ready;
  byId("create-channel").disabled = !ready;
  byId("check-key").disabled = busy;
  for (const button of document.querySelectorAll("[data-setup-action]")) {
    button.disabled = busy || (button.dataset.needsKey === "true" && !signerOk);
  }
}

function runtime() {
  return inventory?.runtimes.find(
    (row) => row.instanceRef === byId("runtime").value,
  );
}

function refusalNode(outcome) {
  return el(
    "p",
    { class: "bad", testid: "refusal" },
    el("code", null, outcome.code || "REFUSED"),
    " ",
    outcome.message || "",
  );
}

async function run(action) {
  return invoke("manage_run", { action });
}

function showResult(...nodes) {
  byId("result").replaceChildren(...nodes);
}

// ── Inventory ────────────────────────────────────────────────────────────────

function providerWords(provider) {
  switch (provider.state) {
    case "live":
      return "provider running";
    case "notSupervised":
      return "provider stopped";
    case "starting":
      return "provider starting";
    default:
      return `provider ${provider.state}${provider.reason ? `: ${provider.reason}` : ""}`;
  }
}

function renderHost() {
  const host = inventory.host;
  byId("host-line").textContent =
    `${host.relayUrl} · ${providerWords(host.provider)} · relay connection unknown to the Host · owner ${
      host.ownerPubkey
        ? short(host.ownerPubkey)
        : `unverified: ${host.ownerUnverified}`
    }`;
  byId("notices").replaceChildren(
    ...(inventory.warnings || []).map((warning) =>
      el(
        "div",
        { class: "notice warn" },
        el("code", null, warning.code),
        " ",
        warning.message,
      ),
    ),
  );
}

const READINESS = {
  adapter_answered: ["good", "adapter answered"],
  unconfirmed: ["warn", "not probed"],
  discovery_failed: ["bad", "adapter did not answer"],
  credentials_unavailable: ["bad", "credential unavailable"],
  not_offered: ["bad", "not offered"],
  catalog_stale: ["bad", "catalog stale"],
  catalog_unavailable: ["bad", "catalog unavailable"],
  provider_not_live: ["bad", "provider not running"],
};

function renderRuntimes() {
  const rows = inventory.runtimes.map((row) => {
    const [tone, word] = READINESS[row.readiness] || ["warn", row.readiness];
    return el(
      "tr",
      { testid: `runtime-${row.instanceRef}` },
      el("td", null, el("code", null, row.instanceRef), el("br"), row.driver),
      el(
        "td",
        null,
        el("span", { class: `badge ${tone}` }, word),
        el("p", { class: "muted" }, row.readinessDetail),
      ),
      el(
        "td",
        null,
        row.selectableModels.length
          ? row.selectableModels
              .map((model) => model.id + (model.isDefault ? " (default)" : ""))
              .join(", ")
          : el("span", { class: "muted" }, "nothing on offer now"),
      ),
      el(
        "td",
        null,
        row.commissionedModels.join(", "),
        row.commissionedNotOffered.length
          ? el(
              "p",
              { class: "warn" },
              `Commissioned, not offered: ${row.commissionedNotOffered.join(", ")}`,
            )
          : null,
      ),
    );
  });
  byId("runtimes").replaceChildren(
    rows.length
      ? el(
          "div",
          { class: "scroll" },
          el(
            "table",
            null,
            el(
              "thead",
              null,
              el(
                "tr",
                null,
                ...[
                  "Runtime",
                  "Readiness",
                  "Effective catalog",
                  "Commissioned",
                ].map((h) => el("th", null, h)),
              ),
            ),
            el("tbody", null, ...rows),
          ),
        )
      : el("p", { class: "muted" }, "No runtime is commissioned on this Host."),
  );
}

function renderAgents() {
  const rows = inventory.agents.map((row) =>
    el(
      "tr",
      null,
      el(
        "td",
        null,
        row.displayName ||
          el("span", { class: "muted" }, "name not known to the Host"),
        el("br"),
        el("code", null, short(row.actor)),
      ),
      el("td", null, row.roles.join(", ")),
      el("td", null, row.runtimes.join(", ")),
      el(
        "td",
        null,
        row.projects.map((project) => project.split(":").pop()).join(", "),
      ),
      el(
        "td",
        null,
        `${row.openSessions} open of ${row.sessionCount}${row.turnsInFlight ? `, ${row.turnsInFlight} working` : ""}`,
      ),
      el(
        "td",
        null,
        row.custody === "host_enrolled" ? "Host" : "sessions only",
      ),
    ),
  );
  byId("agents").replaceChildren(
    rows.length
      ? el(
          "div",
          { class: "scroll" },
          el(
            "table",
            null,
            el(
              "thead",
              null,
              el(
                "tr",
                null,
                ...[
                  "Agent",
                  "Roles",
                  "Runtime",
                  "Projects",
                  "Sessions",
                  "Custody",
                ].map((h) => el("th", null, h)),
              ),
            ),
            el("tbody", null, ...rows),
          ),
        )
      : el("p", { class: "muted" }, "This Host has no agents."),
  );
}

function renderRecovery() {
  const recovery = inventory.recovery;
  if (recovery.unavailable) {
    byId("recovery").replaceChildren(
      el("p", { class: "bad" }, recovery.unavailable),
    );
    return;
  }
  if (!recovery.rows.length) {
    byId("recovery").replaceChildren(
      el("p", { class: "muted" }, "No seat is waiting for custody."),
    );
    return;
  }
  const groups = new Map();
  for (const row of recovery.rows) {
    if (!groups.has(row.code)) groups.set(row.code, []);
    groups.get(row.code).push(row);
  }
  byId("recovery").replaceChildren(
    el(
      "p",
      null,
      `${recovery.rows.length} request(s) waiting. `,
      el("span", { class: "muted" }, recovery.writeOperation),
    ),
    ...[...groups].map(([code, rows]) =>
      el(
        "details",
        { testid: `recovery-${code}` },
        el("summary", null, el("code", null, code), ` · ${rows.length}`),
        el("p", null, rows[0].remedy),
        el(
          "div",
          { class: "scroll" },
          el(
            "table",
            null,
            el(
              "thead",
              null,
              el(
                "tr",
                null,
                ...[
                  "Agent",
                  "Role",
                  "Project",
                  "Session",
                  "To stop this one",
                ].map((h) => el("th", null, h)),
              ),
            ),
            el(
              "tbody",
              null,
              ...rows.map((row) =>
                el(
                  "tr",
                  null,
                  el("td", null, el("code", null, short(row.actor))),
                  el("td", null, row.role || "none"),
                  el(
                    "td",
                    null,
                    row.projectRef ? row.projectRef.split(":").pop() : "none",
                  ),
                  el(
                    "td",
                    null,
                    el(
                      "code",
                      null,
                      `${short(row.sessionId)} g${row.generation}`,
                    ),
                  ),
                  el(
                    "td",
                    null,
                    row.stopCommand
                      ? el("code", null, row.stopCommand)
                      : el("span", { class: "muted" }, "channel not known"),
                  ),
                ),
              ),
            ),
          ),
        ),
      ),
    ),
  );
}

const SETUP_WORDS = {
  planned: ["warn", "planned"],
  applying: ["warn", "working now"],
  interrupted: ["bad", "interrupted, retry to resume"],
  running: ["good", "running"],
  provider_not_running: ["bad", "not running: the session provider is stopped"],
  not_held: ["bad", "not running: its session has closed"],
  cancelled_held: ["bad", "cancelled, and still running"],
  stopped: ["warn", "stopped"],
  needs_attention: ["bad", "needs attention"],
  cancelled: ["warn", "cancelled"],
};

function setupAction(label, setupId, needsKey, handler, primary) {
  const button = el(
    "button",
    {
      type: "button",
      class: primary ? "primary" : null,
      testid: `${label.toLowerCase().split(" ")[0]}-${setupId}`,
    },
    label,
  );
  button.dataset.setupAction = "true";
  button.dataset.needsKey = String(needsKey);
  button.addEventListener("click", handler);
  return button;
}

function renderSetups() {
  const rows = inventory.setups.map(
    ({ setup, effectiveState, executionHeld }) => {
      const [tone, word] = SETUP_WORDS[effectiveState] || [
        "warn",
        effectiveState,
      ];
      const id = setup.setupId;
      const actions = [];
      const key = () => keyFile();
      if (
        ["planned", "interrupted", "needs_attention"].includes(effectiveState)
      ) {
        actions.push(
          setupAction(
            effectiveState === "planned"
              ? "Apply"
              : setup.pendingAction?.action === "stop"
                ? "Retry stop"
                : "Retry",
            id,
            true,
            () =>
              act({
                kind: "apply",
                setup: id,
                resumeCancelled: false,
                keyFile: key(),
              }),
            true,
          ),
        );
      }
      // Cancel recorded an intent. A held action still has to be finished, and only
      // Resume can do that from here: a stop held reads as what it does.
      const stopHeld = setup.pendingAction?.action === "stop";
      const actionUnknown = setup.pendingAction?.attempted === true;
      if (
        effectiveState === "cancelled" ||
        effectiveState === "cancelled_held"
      ) {
        actions.push(
          setupAction(
            stopHeld ? "Retry stop" : "Resume",
            id,
            true,
            () =>
              act({
                kind: "apply",
                setup: id,
                resumeCancelled: true,
                keyFile: key(),
              }),
            true,
          ),
        );
      }
      if (["stopped", "not_held"].includes(effectiveState)) {
        actions.push(
          setupAction(
            "Start",
            id,
            true,
            () => act({ kind: "start", setup: id, keyFile: key() }),
            true,
          ),
        );
      }
      // Whatever the journal's story, an open execution can be stopped, unless a
      // stop is already held, which Retry stop finishes.
      if ((effectiveState === "running" || executionHeld) && !actionUnknown) {
        actions.push(
          setupAction("Stop", id, true, () =>
            act({ kind: "stop", setup: id, keyFile: key() }),
          ),
        );
      }
      if (
        ["planned", "interrupted", "needs_attention"].includes(effectiveState)
      ) {
        actions.push(
          setupAction("Cancel", id, false, () =>
            act({ kind: "cancel", setup: id }),
          ),
        );
      }
      if (effectiveState === "applying") {
        const interrupt = el(
          "button",
          { type: "button", testid: `interrupt-${id}` },
          "Interrupt",
        );
        interrupt.addEventListener("click", async () => {
          if (!(await invoke("manage_interrupt"))) {
            showResult(
              el(
                "p",
                { class: "warn" },
                "This window is not running that action, so it cannot interrupt it. It was started elsewhere.",
              ),
            );
          }
        });
        actions.push(interrupt);
      }
      if (setup.agentPubkey) {
        actions.push(setupAction("Inspect", id, true, () => inspect(id)));
      }
      const readback = setup.readback;
      return el(
        "div",
        { class: "card", testid: `setup-${id}` },
        el(
          "header",
          null,
          el("h3", null, setup.spec.displayName),
          el("span", { class: `badge ${tone}`, testid: "setup-state" }, word),
          el(
            "span",
            { class: "muted" },
            `${setup.spec.role} · ${setup.spec.project.split(":").pop()} · ${setup.spec.providerInstance} · ${setup.spec.model}`,
          ),
        ),
        setup.lastRefusal
          ? el(
              "p",
              { class: "bad", testid: "setup-refusal" },
              el("code", null, setup.lastRefusal.code),
              " ",
              setup.lastRefusal.message,
              setup.lastRefusal.uncertain
                ? " Its outcome is unknown: Retry resends the same action and cannot repeat it. A provider away for more than a day may never answer, and then nothing here resolves it."
                : "",
            )
          : null,
        setup.attempts?.length
          ? el(
              "p",
              { class: "muted", testid: "setup-attempts" },
              `Earlier attempts closed: ${setup.attempts.map((attempt) => `${attempt.action} ${short(attempt.operationId)} (${attempt.reason})`).join(", ")}.`,
            )
          : null,
        setup.agentPubkey
          ? el(
              "p",
              { class: "muted" },
              "Identity ",
              el("code", { testid: "setup-identity" }, setup.agentPubkey),
            )
          : null,
        setup.projectContext === "bound"
          ? el(
              "p",
              { class: "muted", testid: "setup-context" },
              "Project context bound: it knows this project on every turn, and is told it cannot hire.",
            )
          : setup.projectContext === "not_applicable"
            ? el(
                "p",
                { class: "muted", testid: "setup-context" },
                "No project context: the provider gives that only to a role named lead.",
              )
            : null,
        readback
          ? el(
              "p",
              { class: "muted", testid: "setup-readback" },
              `Signed definition ${short(readback.definition?.eventId)}: model ${readback.definition?.model}, access ${readback.definition?.respondTo}. `,
              readback.provider
                ? `Provider signed model ${readback.provider.model}, status ${readback.provider.status}. `
                : "No provider metadata read. ",
              readback.turnEvidence,
            )
          : null,
        el("div", { class: "row" }, ...actions),
      );
    },
  );
  byId("setups").replaceChildren(
    ...(rows.length ? rows : [el("p", { class: "muted" }, "None yet.")]),
  );
}

// Filling a select keeps what the person chose when it is still on offer.
function fill(select, options, placeholder) {
  const held = select.value;
  select.replaceChildren(
    ...(options.length ? options : [{ value: "", label: placeholder }]).map(
      (option) => el("option", { value: option.value }, option.label),
    ),
  );
  if (options.some((option) => option.value === held)) select.value = held;
}

function fillModels() {
  const row = runtime();
  fill(
    byId("model"),
    (row?.selectableModels || []).map((model) => ({
      value: model.id,
      label:
        (model.name ? `${model.name} (${model.id})` : model.id) +
        (model.isDefault ? ", provider default" : ""),
    })),
    "Nothing on offer now",
  );
  const chosen = row?.selectableModels.find(
    (model) => model.id === byId("model").value,
  );
  const notes = [];
  if (row) notes.push(row.readinessDetail);
  if (chosen?.isLabel)
    notes.push(
      "`default` names no model: the real one is known only from a signed turn result.",
    );
  if (chosen?.efforts?.length)
    notes.push(
      `This runtime has effort settings (${chosen.efforts.join(", ")}); this setup does not set one.`,
    );
  byId("model-note").textContent = notes.join(" ");
  updateControls();
}

function fillForm() {
  fill(
    byId("project"),
    inventory.projects
      .filter((row) => row.usable)
      .map((row) => ({
        value: row.coordinate,
        label: row.coordinate.split(":").pop(),
      })),
    "No usable project on this Host",
  );
  fill(
    byId("runtime"),
    inventory.runtimes.map((row) => ({
      value: row.instanceRef,
      label: `${row.instanceRef}${row.selectableModels.length ? "" : " (nothing on offer)"}`,
    })),
    "No runtime commissioned",
  );
  fillModels();
}

async function loadInventory() {
  const outcome = await invoke("manage_inventory");
  if (!outcome.ok) {
    inventory = null;
    byId("host-line").textContent =
      "The Host did not answer, so nothing below is current.";
    byId("notices").replaceChildren(
      el(
        "div",
        { class: "notice bad", testid: "host-refusal" },
        el("code", null, outcome.code),
        " ",
        outcome.message,
      ),
    );
    for (const id of ["setups", "runtimes", "agents", "recovery"])
      byId(id).replaceChildren();
    updateControls();
    return;
  }
  inventory = outcome.value;
  renderHost();
  renderRuntimes();
  renderAgents();
  renderRecovery();
  renderSetups();
  if (document.activeElement?.tagName !== "SELECT") fillForm();
  updateControls();
}

// ── Actions ──────────────────────────────────────────────────────────────────

async function act(action) {
  setBusy(true);
  showResult(el("p", { class: "muted" }, "Working…"));
  try {
    const outcome = await run(action);
    if (outcome.ok) {
      const residual = outcome.value?.residual;
      showResult(
        residual
          ? el("p", { testid: "result-note" }, residual.next)
          : el("p", { class: "good", testid: "result-note" }, "Done."),
      );
    } else {
      showResult(refusalNode(outcome));
    }
    return outcome;
  } finally {
    setBusy(false);
    await loadInventory();
  }
}

async function inspect(setupId) {
  const outcome = await run({
    kind: "inspect",
    setup: setupId,
    keyFile: keyFile(),
  });
  showResult(
    outcome.ok
      ? el(
          "pre",
          { testid: "inspect-result" },
          el(
            "code",
            null,
            JSON.stringify(
              { state: outcome.value.effectiveState, live: outcome.value.live },
              null,
              2,
            ),
          ),
        )
      : refusalNode(outcome),
  );
}

async function checkKey() {
  setBusy(true);
  try {
    const outcome = await run({ kind: "signer", keyFile: keyFile() });
    signerOk = Boolean(outcome.ok && outcome.value?.matches);
    const signer = byId("signer");
    if (!outcome.ok) {
      signer.className = "bad";
      signer.replaceChildren(
        el("code", null, outcome.code),
        " ",
        outcome.message || "",
      );
    } else if (signerOk) {
      signer.className = "good";
      signer.textContent = `Signs as ${short(outcome.value.signer)}, this Host's owner (${outcome.value.source}).`;
    } else {
      signer.className = "bad";
      signer.replaceChildren(
        el("code", null, "OWNER_SIGNER_MISMATCH"),
        ` This key signs as ${short(outcome.value.signer)}, and this Host's owner is ${short(outcome.value.hostOwner) || "unverified"}. Changes stay disabled. Choose the key file of this Host's owner; Beekeeper does not read the Keychain from here.`,
      );
    }
    if (signerOk) await loadChannels();
  } finally {
    setBusy(false);
  }
}

async function loadChannels() {
  const outcome = await run({ kind: "channels", keyFile: keyFile() });
  if (!outcome.ok) {
    showResult(refusalNode(outcome));
    return;
  }
  fill(
    byId("channel"),
    outcome.value.map((row) => ({
      value: row.id,
      label: `${row.name}${row.private ? " (private)" : ""}`,
    })),
    "No channel you own; create one below",
  );
  updateControls();
}

async function loadRoles() {
  const project = byId("project").value;
  if (!project) {
    fill(byId("role"), [], "Choose a project");
    return;
  }
  const outcome = await run({ kind: "roles", project });
  if (!outcome.ok) {
    fill(byId("role"), [], "Roles could not be read");
    showResult(refusalNode(outcome));
    return;
  }
  fill(
    byId("role"),
    outcome.value.roles.map((role) => ({
      value: role.slug,
      label:
        role.slug +
        (role.declaredModel ? ` (declares ${role.declaredModel})` : ""),
    })),
    "This project's source has no roles",
  );
  if (
    outcome.value.lead &&
    outcome.value.roles.some((role) => role.slug === outcome.value.lead)
  ) {
    byId("role").value = outcome.value.lead;
  }
  updateControls();
}

function checksNode(report) {
  return el(
    "ul",
    { class: "checks", testid: "preflight-checks" },
    ...report.checks.map((check) =>
      el(
        "li",
        { class: check.warning ? "warn" : check.ok ? "good" : "bad" },
        check.warning ? "! " : check.ok ? "✓ " : "✕ ",
        el("code", null, check.code),
        " ",
        check.message,
      ),
    ),
  );
}

async function preflight() {
  setBusy(true);
  try {
    const outcome = await run({
      kind: "preflight",
      spec: spec(),
      keyFile: keyFile(),
    });
    showResult(outcome.ok ? checksNode(outcome.value) : refusalNode(outcome));
  } finally {
    setBusy(false);
  }
}

async function setUp(event) {
  event.preventDefault();
  setBusy(true);
  try {
    const planned = await run({
      kind: "plan",
      spec: spec(),
      keyFile: keyFile(),
    });
    if (!planned.ok) {
      showResult(refusalNode(planned));
      return;
    }
    await loadInventory();
    showResult(
      el(
        "p",
        { class: "muted" },
        "Installing and starting. This can take a minute…",
      ),
    );
    const applied = await run({
      kind: "apply",
      setup: planned.value.setupId,
      resumeCancelled: false,
      keyFile: keyFile(),
    });
    showResult(
      applied.ok
        ? el(
            "p",
            { class: "good", testid: "result-note" },
            "Running. Mention it in its channel to start a conversation; its first answer is the proof it works.",
          )
        : refusalNode(applied),
    );
  } finally {
    setBusy(false);
    await loadInventory();
  }
}

async function trust() {
  const outcome = await act({
    kind: "trust",
    project: byId("trust-project").value.trim(),
    checkout: byId("trust-checkout").value.trim(),
    agentsRepo: byId("trust-agents").value.trim(),
    keyFile: keyFile(),
  });
  if (outcome.ok) await loadRoles();
}

async function createChannel() {
  const outcome = await act({
    kind: "createChannel",
    name: byId("new-channel").value.trim(),
    keyFile: keyFile(),
  });
  if (outcome.ok) {
    await loadChannels();
    const id = outcome.value?.channel_id;
    if (id) byId("channel").value = id;
    updateControls();
  }
}

// ── Wiring ───────────────────────────────────────────────────────────────────

byId("refresh").addEventListener("click", loadInventory);
byId("check-key").addEventListener("click", checkKey);
byId("key-file").addEventListener("input", () => {
  // A different path has not been checked.
  signerOk = false;
  byId("signer").className = "muted";
  byId("signer").textContent = "Not checked yet.";
  updateControls();
});
byId("project").addEventListener("change", loadRoles);
byId("runtime").addEventListener("change", fillModels);
byId("model").addEventListener("change", fillModels);
byId("preflight").addEventListener("click", preflight);
byId("setup-form").addEventListener("submit", setUp);
byId("trust").addEventListener("click", trust);
byId("create-channel").addEventListener("click", createChannel);
for (const radio of document.querySelectorAll('input[name="access"]')) {
  radio.addEventListener("change", () => {
    const allowlist = radio.value === "allowlist" && radio.checked;
    if (radio.checked) {
      byId("allowlist").hidden = !allowlist;
      byId("allowlist-note").hidden = !allowlist;
    }
  });
}

// Reads are cheap and socket-only. Skipped while a write runs, so a setup's own
// journal is what the person is reading when it finishes.
setInterval(() => {
  if (!busy) loadInventory();
}, 5000);
loadInventory().then(loadRoles);
