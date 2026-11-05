// Manage Host's read-only views: shared element helpers, runtimes, other identities,
// seat custody and an agent's technical details. They hold no state of the page and
// take no action. Loaded before manage-host.js, which takes these from
// window.ManageHostView.
(() => {
  const byId = (id) => document.getElementById(id);
  // Which disclosures a person opened: a redraw must not close them.
  const openDetails = new Set();

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

  function keptOpen(details, key) {
    details.open = openDetails.has(key);
    details.addEventListener("toggle", () => {
      if (details.open) openDetails.add(key);
      else openDetails.delete(key);
    });
    return details;
  }

  // WebKit's Tab skips buttons, menus, radios and disclosures unless the system's
  // keyboard setting says otherwise, or the element asks for focus itself. This
  // window has to work from the keyboard either way.
  function tabbable(scope) {
    for (const node of scope.querySelectorAll(
      "button, select, input[type=radio], summary",
    )) {
      node.setAttribute("tabindex", "0");
    }
  }

  // Inventory is read every five seconds and usually says the same thing. Redraw a
  // region only when what it shows changed, and give keyboard focus back when it did.
  const drawn = new Map();
  function draw(region, content, paint) {
    const key = JSON.stringify(content);
    if (drawn.get(region) === key) return;
    drawn.set(region, key);
    const active = document.activeElement;
    const scope = byId(region);
    const focused =
      active && scope.contains(active) && active.dataset.testid
        ? active.dataset.testid
        : null;
    paint();
    tabbable(scope);
    if (focused) {
      scope.querySelector(`[data-testid="${CSS.escape(focused)}"]`)?.focus();
    }
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

  function renderRuntimes(inventory) {
    draw("runtimes", inventory.runtimes, () => {
      const rows = inventory.runtimes.map((row) => {
        const [tone, word] = READINESS[row.readiness] || [
          "warn",
          row.readiness,
        ];
        return el(
          "div",
          { class: "runtime", testid: `runtime-${row.instanceRef}` },
          el(
            "div",
            { class: "runtime-head" },
            el(
              "span",
              null,
              el("code", null, row.instanceRef),
              " ",
              el("span", { class: "muted" }, row.driver),
            ),
            el("span", { class: `badge ${tone}` }, word),
          ),
          el("p", { class: "muted" }, row.readinessDetail),
          row.commissionedNotOffered.length
            ? el(
                "p",
                { class: "warn" },
                `Commissioned, not offered: ${row.commissionedNotOffered.join(", ")}`,
              )
            : null,
          keptOpen(
            el(
              "details",
              null,
              el(
                "summary",
                null,
                row.selectableModels.length
                  ? `${row.selectableModels.length} on offer now`
                  : "Nothing on offer now",
              ),
              row.selectableModels.length
                ? el(
                    "ul",
                    null,
                    ...row.selectableModels.map((model) =>
                      el(
                        "li",
                        null,
                        model.id + (model.isDefault ? " (default)" : ""),
                      ),
                    ),
                  )
                : null,
              el(
                "p",
                { class: "muted" },
                `Commissioned: ${row.commissionedModels.join(", ") || "none"}`,
              ),
            ),
            `runtime:${row.instanceRef}`,
          ),
        );
      });
      byId("runtimes").replaceChildren(
        ...(rows.length
          ? rows
          : [
              el(
                "p",
                { class: "muted" },
                "No runtime is commissioned on this Host.",
              ),
            ]),
      );
    });
  }

  function sessionWords(row) {
    return `${row.openSessions} open of ${row.sessionCount}${row.turnsInFlight ? `, ${row.turnsInFlight} working` : ""}`;
  }

  // A setup's identity is already on its card; this is everything else on the Host.
  function renderAgents(inventory) {
    const others = inventory.agents.filter((row) => !row.setupId);
    draw("agents", others, () => {
      const rows = others.map((row) =>
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
            sessionWords(row),
            row.priorOpenSessions
              ? `, plus ${row.priorOpenSessions} from before adoption${row.priorTurnsInFlight ? `, ${row.priorTurnsInFlight} working` : ""}`
              : "",
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
          ? keptOpen(
              el(
                "details",
                { class: "identities" },
                el(
                  "summary",
                  null,
                  `${rows.length} ${rows.length === 1 ? "identity" : "identities"} with no setup here`,
                ),
                el(
                  "div",
                  {
                    class: "scroll",
                    tabindex: "0",
                    role: "region",
                    "aria-label": "Scrollable table",
                  },
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
                ),
              ),
              "identities",
            )
          : el("p", { class: "muted" }, "None."),
      );
    });
  }

  function renderRecovery(inventory) {
    draw("recovery", inventory.recovery, () => paintRecovery(inventory));
  }

  function paintRecovery(inventory) {
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
        `${recovery.rows.length} ${recovery.rows.length === 1 ? "request" : "requests"} waiting. `,
        el("span", { class: "muted" }, recovery.writeOperation),
      ),
      ...[...groups].map(([code, rows]) =>
        keptOpen(
          el(
            "details",
            { class: "group-rows", testid: `recovery-${code}` },
            el("summary", null, el("code", null, code), ` · ${rows.length}`),
            el("p", null, rows[0].remedy),
            el(
              "div",
              {
                class: "scroll",
                tabindex: "0",
                role: "region",
                "aria-label": "Scrollable table",
              },
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
                        row.projectRef
                          ? row.projectRef.split(":").pop()
                          : "none",
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
          `recovery:${code}`,
        ),
      ),
    );
  }

  // One name with its value, as one grid cell.
  function fact(name, ...value) {
    return el("div", null, el("dt", null, name), el("dd", null, ...value));
  }

  function accessWords(access) {
    if (access?.mode === "allowlist") {
      const people = access.pubkeys?.length || 0;
      return `Owner and ${people} ${people === 1 ? "person" : "people"}`;
    }
    return "Only the owner";
  }

  // What a person needs beside the buttons: why one is absent, and what Start does
  // to the conversation. The evidence for each stays on the journal and the ledger.
  function cardHint(effectiveState, canStop, actionUnknown, executionHeld) {
    if ((effectiveState === "running" || executionHeld) && actionUnknown)
      return "Stop is not offered while an earlier action's outcome is unknown. Finish that first.";
    if (canStop)
      return "Stop ends this session. Starting it again opens a fresh session without this conversation.";
    if (effectiveState === "stopped" || effectiveState === "not_held")
      return "Start opens a fresh session. It will not recall earlier conversations.";
    if (effectiveState === "provider_not_running")
      return "Nothing can be done here until the session provider is running.";
    return null;
  }

  // `model[effort]`, as the spec and the signed definition spell it.
  function selectionOf(spec) {
    return spec.effort ? `${spec.model}[${spec.effort}]` : spec.model;
  }

  // A setup's reading is saved when it applies or starts. A later model change moves
  // the spec and leaves the reading as it was, so the two can honestly disagree.
  function readingOutdated(setup) {
    const named = setup.readback?.definition?.model;
    return Boolean(named) && named !== selectionOf(setup.spec);
  }

  function technicalDetails(row, agent, inspections, channelNames) {
    const { setup } = row;
    const id = setup.setupId;
    const readback = setup.readback;
    const inspection = inspections.get(id);
    const channel = setup.spec.channel;
    const allowlist =
      setup.spec.access?.mode === "allowlist"
        ? setup.spec.access.pubkeys
        : null;
    return keptOpen(
      el(
        "details",
        { class: "technical", testid: `technical-${id}` },
        el("summary", null, "Technical details"),
        el(
          "dl",
          { class: "tech" },
          setup.agentPubkey
            ? fact(
                "Identity",
                el("code", { testid: "setup-identity" }, setup.agentPubkey),
              )
            : null,
          fact("Setup", el("code", null, id)),
          fact("Project", el("code", null, setup.spec.project)),
          fact(
            "Channel",
            channelNames.has(channel) ? `${channelNames.get(channel)} ` : null,
            el("code", null, channel),
          ),
          allowlist
            ? fact(
                "May instruct",
                ...allowlist.flatMap((key) => [
                  el("code", null, key),
                  el("br"),
                ]),
              )
            : null,
          agent ? fact("Custody", agent.custody) : null,
        ),
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
        readingOutdated(setup)
          ? el(
              "p",
              { class: "inline-warning", testid: "setup-readback-outdated" },
              `The reading below names ${readback.definition?.model}, and this setup now requests ${selectionOf(setup.spec)}. It was saved before that change. Inspect reads the signed records again.`,
            )
          : null,
        readback
          ? el(
              "p",
              { class: "muted", testid: "setup-readback" },
              readback.readAt ? `Read ${readback.readAt}. ` : null,
              `Signed definition ${short(readback.definition?.eventId)}: model ${readback.definition?.model}, access ${readback.definition?.respondTo}. `,
              readback.provider
                ? `Provider signed model ${readback.provider.model}, status ${readback.provider.status}. `
                : "No provider metadata read. ",
              readback.turnEvidence,
            )
          : null,
        setup.attempts?.length
          ? el(
              "p",
              { class: "muted", testid: "setup-attempts" },
              `Earlier attempts closed: ${setup.attempts.map((attempt) => `${attempt.action} ${short(attempt.operationId)} (${attempt.reason})`).join(", ")}.`,
            )
          : null,
        inspection
          ? el(
              "pre",
              { testid: "inspect-result", tabindex: "0" },
              el("code", null, JSON.stringify(inspection, null, 2)),
            )
          : null,
      ),
      `setup:${id}`,
    );
  }

  function refusalNode(outcome) {
    return el(
      "p",
      { class: "bad", testid: "refusal", role: "alert" },
      el("code", null, outcome.code || "REFUSED"),
      " ",
      outcome.message || "",
    );
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

  function hostFact(tone, text, title) {
    return el("span", { class: `host-fact ${tone}`, title }, text);
  }

  const SETUP_WORDS = {
    planned: ["warn", "planned"],
    applying: ["warn", "working now"],
    interrupted: ["bad", "interrupted, retry to resume"],
    running: ["good", "running"],
    provider_not_running: [
      "bad",
      "not running: the session provider is stopped",
    ],
    not_held: ["bad", "not running: its session has closed"],
    cancelled_held: ["bad", "cancelled, and still running"],
    stopped: ["warn", "stopped"],
    needs_attention: ["bad", "needs attention"],
    cancelled: ["warn", "cancelled"],
  };

  // A person watching a card sees its badge change. Someone using a screen reader
  // hears it here, once per change, and nothing for what was already true on opening.
  const heardStates = new Map();
  function announceChanges(inventory) {
    const heard = [];
    for (const { setup, effectiveState } of inventory.setups) {
      const before = heardStates.get(setup.setupId);
      if (before !== undefined && before !== effectiveState) {
        const [, word] = SETUP_WORDS[effectiveState] || ["", effectiveState];
        heard.push(`${setup.spec.displayName}: ${word}.`);
      }
      heardStates.set(setup.setupId, effectiveState);
    }
    if (heard.length) byId("announcer").textContent = heard.join(" ");
  }

  window.ManageHostView = {
    SETUP_WORDS,
    announceChanges,
    accessWords,
    cardHint,
    checksNode,
    draw,
    el,
    fact,
    hostFact,
    forgetDrawn: () => drawn.clear(),
    openDetail: (key) => openDetails.add(key),
    providerWords,
    refusalNode,
    renderAgents,
    renderRecovery,
    renderRuntimes,
    sessionWords,
    short,
    tabbable,
    technicalDetails,
  };
})();
