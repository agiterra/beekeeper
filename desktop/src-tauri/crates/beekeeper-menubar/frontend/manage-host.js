// The Manage Host window. Presentation only: it holds no key, no model and no
// setup logic. Every read comes from the Host's inventory and every write is a
// named action that this app runs as its own `bee`. Text from the Host or the
// relay reaches the page through textContent, never as markup.

const invoke = (name, args) => window.__TAURI__.core.invoke(name, args);
const byId = (id) => document.getElementById(id);
const {
  SETUP_WORDS,
  accessWords,
  announceChanges,
  cardHint,
  checksNode,
  draw,
  el,
  fact,
  forgetDrawn,
  hostFact,
  openDetail,
  providerWords,
  refusalNode,
  renderAgents,
  renderRecovery,
  renderRuntimes,
  sessionWords,
  short,
  tabbable,
  technicalDetails,
} = window.ManageHostView;

let inventory = null;
let signerOk = false;
let busy = false;
// The setup the running action belongs to: its card can interrupt it.
let runningSetup = null;
// Why the last key check failed, for the words beside the actions it locks.
let signerMismatch = false;
// The last `inspect` reading per setup, shown on that agent's card.
const inspections = new Map();
// Channel names, known once a checked key has listed them.
const channelNames = new Map();
// A path, never a key: only the file's location is remembered, once it matched.
const KEY_PATH_STORE = "manage-host.owner-key-path";

function keyFile() {
  return byId("key-file").value.trim() || null;
}

function rememberKeyPath(path) {
  try {
    localStorage.setItem(KEY_PATH_STORE, path);
  } catch {
    // Storage can be unavailable. The person types the path again; nothing else changes.
  }
}

function rememberedKeyPath() {
  try {
    return localStorage.getItem(KEY_PATH_STORE) || "";
  } catch {
    return "";
  }
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
    ...(byId("effort").value ? { effort: byId("effort").value } : {}),
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
  const row = runtime();
  const offered = (row?.selectableModels || []).length > 0;
  const ready = signerOk && !busy;
  const effortNeeded = !byId("effort").hidden && !byId("effort").value;
  const named = Boolean(byId("display-name").value.trim());
  const chosen = {
    project: byId("project").value,
    channel: byId("channel").value,
    role: byId("role").value,
  };
  byId("set-up").disabled = !(
    ready &&
    offered &&
    !effortNeeded &&
    named &&
    chosen.project &&
    chosen.channel &&
    chosen.role
  );
  byId("preflight").disabled = !(
    ready &&
    chosen.project &&
    chosen.channel &&
    chosen.role
  );
  byId("trust").disabled = !ready;
  byId("create-channel").disabled = !ready;
  byId("check-key").disabled = busy;
  byId("check-key-now").disabled = busy;
  const keyReason = signerMismatch
    ? "this key is not the Host's owner"
    : "check the owner key";
  const missing = [
    !signerOk && keyReason,
    !chosen.project && "choose a project",
    !chosen.role && "choose a role",
    !chosen.channel && "choose a channel",
    !offered && "a runtime with models on offer",
    effortNeeded && "choose an effort",
    !named && "name the agent",
  ].filter(Boolean);
  byId("setup-blocked").textContent = busy
    ? "Another action is running."
    : missing.length
      ? `Still needed: ${missing.join(", ")}.`
      : "";
  byId("extras-blocked").textContent = signerOk
    ? ""
    : "Recording a project or creating a channel needs the owner key.";
  for (const button of document.querySelectorAll("[data-setup-action]")) {
    const needsKey = button.dataset.needsKey === "true" && !signerOk;
    button.disabled = busy || needsKey;
    button.title = busy
      ? "Another action is running."
      : needsKey
        ? "Check the owner key first."
        : "";
  }
  updateKeyWords();
}

function updateKeyWords() {
  const status = byId("key-status");
  status.className = `key-status ${signerOk ? "good" : signerMismatch ? "bad" : "warn"}`;
  status.textContent = signerOk
    ? "Owner key checked"
    : signerMismatch
      ? "Key is not the owner's"
      : "Owner key not checked";
  byId("lock-note").hidden = signerOk;
  byId("lock-note").className = `lock-note${signerMismatch ? " bad" : ""}`;
  byId("lock-words").textContent = signerMismatch
    ? "That key is not this Host's owner, so changes stay locked."
    : "Start, Stop and setup need the owner key.";
  byId("check-key-now").textContent = signerMismatch
    ? "Choose another key"
    : "Check key";
}

function runtime() {
  return inventory?.runtimes.find(
    (row) => row.instanceRef === byId("runtime").value,
  );
}

async function run(action) {
  return invoke("manage_run", { action });
}

function showResult(...nodes) {
  byId("result").replaceChildren(...nodes);
  placeResult();
  if (flowOpen()) byId("result").scrollIntoView({ block: "nearest" });
  else if (nodes.length)
    byId("result-home").scrollIntoView({ block: "nearest" });
}

// One result element, with one id: inside the Add agent panel while that is open,
// so a refusal lands beside the form that caused it, otherwise above the agents.
function placeResult() {
  const result = byId("result");
  const home = byId("result-home");
  if (flowOpen()) {
    byId("flow-result-slot").append(result);
    home.hidden = true;
  } else {
    home.prepend(result);
    home.hidden = !result.childNodes.length;
  }
}

function flowOpen() {
  return !byId("add-flow").hidden;
}

function openFlow() {
  byId("add-flow").hidden = false;
  byId("add-agent").setAttribute("aria-expanded", "true");
  placeResult();
  showFullChoice(byId("model"), byId("model-full"));
  byId("add-flow").scrollIntoView({ block: "nearest" });
  byId("project").focus();
}

function closeFlow() {
  byId("add-flow").hidden = true;
  byId("add-agent").setAttribute("aria-expanded", "false");
  placeResult();
  byId("add-agent").focus();
}

// ── Inventory ────────────────────────────────────────────────────────────────

const PROVIDER_TONE = {
  live: "good",
  starting: "warn",
};

function renderHost() {
  const host = inventory.host;
  const words = providerWords(host.provider);
  byId("host-line").replaceChildren(
    hostFact(
      `state ${PROVIDER_TONE[host.provider.state] || "bad"}`,
      words[0].toUpperCase() + words.slice(1),
    ),
    hostFact("", host.relayUrl.replace(/^wss?:\/\//, ""), host.relayUrl),
    hostFact(
      "",
      "Relay link unknown to the Host",
      host.relayConnection?.why || null,
    ),
    host.ownerPubkey
      ? hostFact("", `Owner ${short(host.ownerPubkey)}`, host.ownerPubkey)
      : hostFact("bad", "Owner unverified", host.ownerUnverified),
  );
  byId("notices").replaceChildren(
    ...(host.ownerPubkey
      ? []
      : [
          el(
            "div",
            { class: "notice bad", testid: "owner-unverified", role: "alert" },
            `The Host's owner is unverified: ${host.ownerUnverified}`,
          ),
        ]),
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

function setupAction(label, setupId, needsKey, handler, tone, apart) {
  const button = el(
    "button",
    {
      type: "button",
      class: [tone, apart ? "apart" : null].filter(Boolean).join(" ") || null,
      testid: `${label.toLowerCase().split(" ")[0]}-${setupId}`,
    },
    label,
  );
  button.dataset.setupAction = "true";
  button.dataset.needsKey = String(needsKey);
  button.addEventListener("click", handler);
  return button;
}

function setupActions(row) {
  const { setup, effectiveState, executionHeld } = row;
  const id = setup.setupId;
  const actions = [];
  const key = () => keyFile();
  const retryable = ["planned", "interrupted", "needs_attention"].includes(
    effectiveState,
  );
  if (retryable) {
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
        "primary",
      ),
    );
  }
  // Cancel recorded an intent. A held action still has to be finished, and only
  // Resume can do that from here: a stop held reads as what it does.
  const stopHeld = setup.pendingAction?.action === "stop";
  const actionUnknown = setup.pendingAction?.attempted === true;
  if (effectiveState === "cancelled" || effectiveState === "cancelled_held") {
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
        "primary",
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
        "primary",
      ),
    );
  }
  if (setup.agentPubkey) {
    actions.push(setupAction("Inspect", id, true, () => inspect(id)));
  }
  // Whatever the journal's story, an open execution can be stopped, unless a
  // stop is already held, which Retry stop finishes.
  const canStop =
    (effectiveState === "running" || executionHeld) && !actionUnknown;
  if (canStop) {
    actions.push(
      setupAction(
        "Stop",
        id,
        true,
        () => act({ kind: "stop", setup: id, keyFile: key() }),
        "danger",
        true,
      ),
    );
  }
  if (retryable) {
    actions.push(
      setupAction(
        "Cancel",
        id,
        false,
        () => act({ kind: "cancel", setup: id }),
        "danger",
        !canStop,
      ),
    );
  }
  if (effectiveState === "applying" || (busy && runningSetup === id)) {
    const interrupt = el(
      "button",
      {
        type: "button",
        class: `danger${canStop || retryable ? "" : " apart"}`,
        testid: `interrupt-${id}`,
      },
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
  return {
    actions,
    hint: cardHint(effectiveState, canStop, actionUnknown, executionHeld),
  };
}

function agentCard(row) {
  const { setup, effectiveState } = row;
  const id = setup.setupId;
  const [tone, word] = SETUP_WORDS[effectiveState] || ["warn", effectiveState];
  const agent = inventory.agents.find(
    (candidate) =>
      candidate.setupId === id ||
      (setup.agentPubkey && candidate.actor === setup.agentPubkey),
  );
  const { actions, hint } = setupActions(row);
  // "Stop" alone says nothing to someone who cannot see which card it is on.
  for (const button of actions) {
    button.setAttribute(
      "aria-label",
      `${button.textContent} ${setup.spec.displayName}`,
    );
  }
  const spec = setup.spec;
  return el(
    "article",
    {
      class: "card",
      "data-tone": tone,
      testid: `setup-${id}`,
      "aria-labelledby": `agent-name-${id}`,
    },
    el(
      "header",
      null,
      el("h3", { id: `agent-name-${id}` }, spec.displayName),
      el("span", { class: `badge ${tone}`, testid: "setup-state" }, word),
      effectiveState === "applying" && setup.steps?.length
        ? el(
            "span",
            { class: "step", testid: "setup-step" },
            `step: ${setup.steps[setup.steps.length - 1].step}`,
          )
        : null,
    ),
    el(
      "dl",
      { class: "facts" },
      fact("Project", spec.project.split(":").pop()),
      fact("Role", spec.role),
      fact("Runtime", spec.providerInstance),
      fact("Model", spec.model, el("span", { class: "tag" }, "requested")),
      fact(
        "Effort",
        spec.effort || "None chosen",
        spec.effort ? el("span", { class: "tag" }, "requested") : null,
      ),
      fact("Who may instruct", accessWords(spec.access)),
      agent ? fact("Sessions", sessionWords(agent)) : null,
    ),
    agent?.priorOpenSessions
      ? el(
          "p",
          { class: "card-hint", testid: "setup-prior" },
          `Plus ${agent.priorOpenSessions} open from before adoption${agent.priorTurnsInFlight ? `, ${agent.priorTurnsInFlight} working` : ""}. Those are not this Host's to stop.`,
        )
      : null,
    setup.modelChange
      ? el(
          "p",
          { class: "inline-warning", testid: "setup-model-change" },
          `A change to ${setup.modelChange.model} is still being recorded, so the model above may not be what its execution uses. Mentions can be refused until it finishes. Run this setup's \`bee host manage set-model\` again to finish it.`,
        )
      : null,
    setup.lastRefusal
      ? el(
          "p",
          { class: "inline-warning bad", testid: "setup-refusal" },
          el("code", null, setup.lastRefusal.code),
          " ",
          setup.lastRefusal.message,
          setup.lastRefusal.uncertain
            ? " Its outcome is unknown. Retry resends the same action and cannot repeat it. A provider away for over a day may never answer, and then nothing here resolves it."
            : "",
        )
      : null,
    hint ? el("p", { class: "card-hint", testid: "setup-hint" }, hint) : null,
    el("div", { class: "card-actions" }, ...actions),
    technicalDetails(row, agent, inspections, channelNames),
  );
}

// Trouble first, then what is running, then the rest. Sorted by name within each
// so the cards do not move around between reads.
function cardRank({ effectiveState }) {
  const [tone] = SETUP_WORDS[effectiveState] || ["warn"];
  if (tone === "bad") return 0;
  return effectiveState === "running" || effectiveState === "applying" ? 1 : 2;
}

function renderSetups() {
  announceChanges(inventory);
  const content = {
    setups: inventory.setups,
    agents: inventory.agents.filter((row) => row.setupId),
    running: busy ? runningSetup : null,
    inspections: [...inspections],
    channels: [...channelNames],
  };
  draw("setups", content, () => {
    const sorted = [...inventory.setups].sort(
      (a, b) =>
        cardRank(a) - cardRank(b) ||
        a.setup.spec.displayName.localeCompare(b.setup.spec.displayName),
    );
    byId("setups").replaceChildren(
      ...(sorted.length
        ? sorted.map(agentCard)
        : [
            el(
              "div",
              { class: "empty", testid: "no-agents" },
              el("p", { class: "muted" }, "No agents are set up on this Host."),
              el(
                "button",
                {
                  type: "button",
                  class: "primary",
                  testid: "add-agent-empty",
                  onclick: openFlow,
                },
                "Add agent",
              ),
            ),
          ]),
    );
  });
  const running = inventory.setups.filter(
    (row) => row.effectiveState === "running",
  ).length;
  const count = inventory.setups.length;
  byId("agents-summary").textContent = count
    ? `${count} ${count === 1 ? "agent" : "agents"}, ${running} running`
    : "";
  byId("model-truth").hidden = !count;
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

// A menu shows only what fits its width. When the chosen model's label does not,
// it is written out in full: this choice decides what runs.
const measure = document.createElement("canvas").getContext("2d");
function showFullChoice(select, line) {
  const label = select.selectedOptions[0]?.textContent || "";
  const style = getComputedStyle(select);
  measure.font = `${style.fontSize} ${style.fontFamily}`;
  const room =
    select.clientWidth -
    Number.parseFloat(style.paddingLeft) -
    Number.parseFloat(style.paddingRight);
  line.textContent = label;
  line.hidden = measure.measureText(label).width <= room;
}

function fillModels() {
  const row = runtime();
  fill(
    byId("model"),
    (row?.selectableModels || []).map((model) => ({
      value: model.id,
      label:
        (model.name
          ? model.isLabel
            ? model.name
            : `${model.name} (${model.id})`
          : model.id) + (model.isDefault ? ", provider default" : ""),
    })),
    "Nothing on offer now",
  );
  const chosen = row?.selectableModels.find(
    (model) => model.id === byId("model").value,
  );
  // The catalog's own values for this model. Nothing is chosen for the person, and
  // the choice is sent exactly: `default` is the adapter deciding, by that name.
  const efforts = chosen?.efforts || [];
  fill(
    byId("effort"),
    efforts.length
      ? [
          { value: "", label: "Choose an effort" },
          ...efforts.map((effort) => ({ value: effort, label: effort })),
        ]
      : [],
    "This model lists none",
  );
  byId("effort").hidden = !efforts.length;
  byId("effort-label").hidden = !efforts.length;
  const notes = [];
  if (row) notes.push(row.readinessDetail);
  if (chosen?.isLabel)
    notes.push(
      "`default` names no model: the real one is known only from a signed turn result.",
    );
  if (efforts.length)
    notes.push(
      "No runtime reports the effort a turn ran at: the provider's signed model records what it applied.",
    );
  showFullChoice(byId("model"), byId("model-full"));
  byId("model-note").replaceChildren(
    ...notes.map((note) => el("p", null, note)),
  );
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
    forgetDrawn();
    byId("host-line").replaceChildren(
      hostFact(
        "state bad",
        "The Host did not answer, so nothing below is current.",
      ),
    );
    byId("agents-summary").textContent = "";
    byId("model-truth").hidden = true;
    byId("notices").replaceChildren(
      el(
        "div",
        { class: "notice bad", testid: "host-refusal", role: "alert" },
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
  renderRuntimes(inventory);
  renderAgents(inventory);
  renderRecovery(inventory);
  renderSetups();
  if (document.activeElement?.tagName !== "SELECT") fillForm();
  updateControls();
}

// ── Actions ──────────────────────────────────────────────────────────────────

async function act(action) {
  runningSetup = action.setup || null;
  setBusy(true);
  lastCards = "";
  refreshCards();
  if (inventory) renderSetups();
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
    runningSetup = null;
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
  if (!outcome.ok) {
    showResult(refusalNode(outcome));
    return;
  }
  // The reading belongs to this agent, so it opens on its card.
  inspections.set(setupId, {
    state: outcome.value.effectiveState,
    live: outcome.value.live,
  });
  openDetail(`setup:${setupId}`);
  renderSetups();
  updateControls();
}

async function checkKey() {
  setBusy(true);
  try {
    const outcome = await run({ kind: "signer", keyFile: keyFile() });
    signerOk = Boolean(outcome.ok && outcome.value?.matches);
    signerMismatch = Boolean(outcome.ok && !outcome.value?.matches);
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
    if (signerOk) {
      rememberKeyPath(keyFile() || "");
      await loadChannels();
    }
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
  channelNames.clear();
  for (const row of outcome.value) channelNames.set(row.id, row.name);
  if (inventory) renderSetups();
  fill(
    byId("channel"),
    outcome.value.map((row) => ({
      value: row.id,
      label: `${row.name}${row.private ? " (private)" : ""}`,
    })),
    "No channel you own; add one below",
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
  let setupExists = false;
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
    setupExists = true;
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
    // It exists now, with its own card and its own Retry: the form's work is done.
    if (setupExists) {
      byId("display-name").value = "";
      closeFlow();
      updateControls();
    }
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
function goToKey() {
  byId("key-file").scrollIntoView({ block: "center" });
  byId("key-file").focus();
}
byId("add-agent").addEventListener("click", () =>
  flowOpen() ? closeFlow() : openFlow(),
);
byId("close-add").addEventListener("click", closeFlow);
byId("add-flow").addEventListener("keydown", (event) => {
  if (event.key === "Escape") closeFlow();
});
byId("key-status").addEventListener("click", goToKey);
// An empty path is a real choice: `bee` then reads ~/.nostr/key, as the field shows.
byId("check-key-now").addEventListener("click", () =>
  signerMismatch ? goToKey() : checkKey(),
);
byId("dismiss-result").addEventListener("click", () => showResult());
byId("display-name").addEventListener("input", updateControls);
byId("key-file").addEventListener("input", () => {
  // A different path has not been checked.
  signerOk = false;
  signerMismatch = false;
  byId("signer").className = "muted";
  byId("signer").textContent = "Not checked yet.";
  updateControls();
});
byId("project").addEventListener("change", loadRoles);
byId("runtime").addEventListener("change", fillModels);
byId("model").addEventListener("change", fillModels);
byId("effort").addEventListener("change", updateControls);
byId("role").addEventListener("change", updateControls);
byId("channel").addEventListener("change", updateControls);
byId("preflight").addEventListener("click", preflight);
byId("setup-form").addEventListener("submit", setUp);
byId("trust").addEventListener("click", trust);
byId("create-channel").addEventListener("click", createChannel);
for (const radio of document.querySelectorAll('input[name="access"]')) {
  radio.addEventListener("change", () => {
    const allowlist = radio.value === "allowlist" && radio.checked;
    if (radio.checked) {
      byId("allowlist").hidden = !allowlist;
      byId("allowlist-label").hidden = !allowlist;
      byId("allowlist-note").hidden = !allowlist;
    }
  });
}

// Reads are cheap and socket-only. While a write runs only the setup cards are
// redrawn, and only when what they say changed: the form keeps its focus and typing,
// and a button under the pointer is not replaced between press and release.
let lastCards = "";
async function refreshCards() {
  const outcome = await invoke("manage_inventory");
  if (!outcome.ok || !busy) return;
  const cards = JSON.stringify(
    outcome.value.setups.map(({ setup, effectiveState }) => [
      setup.setupId,
      effectiveState,
      setup.steps?.[setup.steps.length - 1],
    ]),
  );
  if (cards === lastCards) return;
  lastCards = cards;
  inventory = outcome.value;
  renderSetups();
  updateControls();
}
let lastRead = 0;
setInterval(() => {
  const now = Date.now();
  if (busy) {
    refreshCards();
  } else if (now - lastRead >= 5000) {
    lastRead = now;
    lastCards = "";
    loadInventory();
  }
}, 1000);
tabbable(document);
byId("key-file").value = rememberedKeyPath();
fill(byId("channel"), [], "Check the owner key to list your channels");
updateKeyWords();
loadInventory().then(loadRoles);
