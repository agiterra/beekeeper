import { defineConfig, devices } from "@playwright/test";

import { PREVIEW_ORIGIN, PREVIEW_PORT } from "./tests/helpers/previewOrigin";

// Several worktrees are usually checked out at once and `reuseExistingServer`
// will happily adopt whichever server already answers on the port — serving a
// *sibling's* `dist` against this worktree's specs, which fails as though the
// code under test were broken. So the port is per-worktree: `E2E_PORT` if set,
// else derived from this checkout's path (4173 under CI). See
// `tests/helpers/previewOrigin.ts`.
const PREVIEW_URL = PREVIEW_ORIGIN;

// The mock smoke project runs four files at once locally (each file stays on
// one worker, in order — `fullyParallel` is off): the full project took 27 min
// at 4 workers on 2026-10-04 against 1.6 h at 1 (ledger 311(s)). CI keeps 1
// until a nightly proves the container can take more. Override with
// `E2E_WORKERS=<n>`. The relay-backed integration project shares one database
// and stays at one worker regardless.
function smokeWorkers(): number {
  const raw = process.env.E2E_WORKERS;
  if (raw === undefined || raw.trim() === "") return process.env.CI ? 1 : 4;
  const n = Number(raw);
  if (!Number.isInteger(n) || n < 1) {
    throw new Error(`E2E_WORKERS must be a positive integer, got "${raw}"`);
  }
  return n;
}
const SMOKE_WORKERS = smokeWorkers();

const SMOKE_USE = {
  ...devices["Desktop Chrome"],
  // Chromium denies `navigator.clipboard.write`/`writeText` unless the
  // context is granted these. The Tauri webview the app actually ships in
  // does not, so without the grant the harness tests a permission state
  // production never sees: the copy path rejects, and specs fail on
  // symptoms (`Copy link` never flips to `Copied`) that no user hits.
  permissions: ["clipboard-read", "clipboard-write"],
};

export default defineConfig({
  testDir: "./tests/e2e",
  timeout: 30_000,
  retries: process.env.CI ? 2 : 0,
  workers: SMOKE_WORKERS,
  reporter: [
    ["list"],
    ["html", { open: "never", outputFolder: "playwright-report" }],
  ],
  use: {
    baseURL: PREVIEW_URL,
    screenshot: "only-on-failure",
    trace: "on-first-retry",
    video: "retain-on-failure",
  },
  projects: [
    {
      name: "smoke",
      testMatch: [
        "**/smoke.spec.ts",
        "**/dashboard.spec.ts",
        "**/dashboard-relay-health.spec.ts",
        "**/sidebar-offcanvas-rail.spec.ts",
        "**/search-scope-screenshots.spec.ts",
        "**/coding-sessions.spec.ts",
        // Built-in shell: a tab's scrollback replay never answers into the PTY.
        "**/shell-terminal-replay.spec.ts",
        "**/coding-session-mission-lens.spec.ts",
        // NIP-PW: the native work-coverage projection across the bridge.
        "**/project-work.spec.ts",
        "**/coding-session-mission-density.spec.ts",
        "**/coding-session-observations.spec.ts",
        // Absent-participant handover: claim, reconstruct, fence, deletion.
        "**/coding-session-handover.spec.ts",
        "**/coding-session-founder-acts.spec.ts",
        "**/crew-front-door.spec.ts",
        // LANE-L33 — the shared home disclosure and the nest remedy.
        "**/agent-shared-home-nest.spec.ts",
        // LANE-L25 — the join dialog's projectRef wiring.
        "**/l25-provider-project-ref.spec.ts",
        "**/role-packs-project.spec.ts",
        "**/project-agents-tab.spec.ts",
        "**/project-agents-repo.spec.ts",
        "**/memory-explorer.spec.ts",
        // Project agents and hiring: association, borrowed, lead picker, roster.
        "**/project-agent-hiring.spec.ts",
        "**/coding-session-reachability.spec.ts",
        "**/coding-session-model-picker.spec.ts",
        "**/coding-session-capacity.spec.ts",
        "**/coding-session-goal.spec.ts",
        "**/coding-session-founded-setup.spec.ts",
        // SV-31: a generated title's marker, rename and foreign-signer cases.
        "**/coding-session-auto-title.spec.ts",
        // SV-69 / SV-70: tab-strip keyboard and names; header Auto-named.
        "**/coding-session-sv69-70-header.spec.ts",
        // SV-56 / D9: the session-title mode card in each mode, and the
        // founded flow honouring it.
        "**/coding-session-title-mode-settings.spec.ts",
        "**/coding-session-title-mode-founded.spec.ts",
        "**/coding-session-transcript-narrative-screenshots.spec.ts",
        // The provider's project execution boundary disclosure, both states.
        "**/coding-session-project-boundary.spec.ts",
        // Session-view parity wave A: one hash-distinct shot per UI ID, and
        // the mission view's sandbox chip (SV-17).
        "**/coding-session-parity-screenshots.spec.ts",
        "**/coding-session-umbrella-sandbox.spec.ts",
        // Session-view parity wave B: every lane's spec, listed by name
        // (check:e2e-registration reads literal file names, not globs).
        "**/coding-session-wave-b-audit-markdown.spec.ts",
        "**/coding-session-wave-b-audit-transcript-model.spec.ts",
        "**/coding-session-wave-b-audit-transcript-view.spec.ts",
        "**/coding-session-subagent-rows.spec.ts",
        "**/coding-session-sv78-background-task.spec.ts",
        // SV-118: retained projection — late and patching events on screen.
        "**/coding-session-sv118-streaming.spec.ts",
        "**/coding-session-liveness.spec.ts",
        "**/coding-session-subagent-page.spec.ts",
        "**/coding-session-wave-b-audit-workspace.spec.ts",
        "**/coding-session-wave-b-followups.spec.ts",
        "**/coding-session-wave-b-registry.spec.ts",
        "**/coding-session-wave-b-header.spec.ts",
        "**/coding-session-wave-b-surfaces.spec.ts",
        "**/coding-session-wave-b-badges.spec.ts",
        "**/coding-session-wave-b-terminal.spec.ts",
        "**/coding-session-wave-b-minimap.spec.ts",
        "**/coding-session-wave-b-orchestration.spec.ts",
        "**/coding-session-terminal-shortcut.spec.ts",
        "**/coding-session-elision-screenshots.spec.ts",
        "**/coding-session-sv75-command-glyph.spec.ts",
        "**/coding-session-seat-bee.spec.ts",
        "**/coding-session-surface-host-screenshots.spec.ts",
        "**/coding-session-connect.spec.ts",
        // Native steering: turn_injected / turn_delivery_unknown on the row.
        "**/coding-session-native-steer.spec.ts",
        "**/coding-session-short-window.spec.ts",
        "**/coding-session-paging.spec.ts",
        "**/coding-session-width.spec.ts",
        "**/coding-session-worktree-source.spec.ts",
        // Lane V — "New session in this workspace": reuse, refusal, no memory.
        "**/coding-session-workspace-reuse.spec.ts",
        // L11 (worktree lifecycle) — registration line only.
        "**/coding-session-worktree-closure.spec.ts",
        // Lane V — People/Agents selection clarity and the session roster.
        "**/people-setup.spec.ts",
        "**/onboarding-docked-cta-screenshots.spec.ts",
        "**/identity-key-help.spec.ts",
        "**/key-import-reveal.spec.ts",
        "**/navigation.spec.ts",
        "**/nav-hotkeys.spec.ts",
        "**/channels.spec.ts",
        "**/channel-shared-header-backdrop.spec.ts",
        "**/channel-composer-overflow.spec.ts",
        "**/badge.spec.ts",
        "**/channel-browser.spec.ts",
        "**/channel-add-screenshots.spec.ts",
        "**/add-community-screenshots.spec.ts",
        "**/invites-settings-screenshots.spec.ts",
        "**/messaging.spec.ts",
        "**/message-feedback-snapshots.spec.ts",
        "**/custom-emoji.spec.ts",
        "**/profile-custom-emoji-status.spec.ts",
        "**/custom-emoji-ui.spec.ts",
        "**/channel-mute.spec.ts",
        "**/channel-star.spec.ts",
        "**/channel-controls.spec.ts",
        "**/channel-activity-popover.spec.ts",
        "**/active-turn-resilience.spec.ts",
        "**/agent-control-regressions.spec.ts",
        "**/profile-active-turn.spec.ts",
        "**/config-bridge-screenshots.spec.ts",
        "**/observer-feed-screenshots.spec.ts",
        "**/core-memory-screenshots.spec.ts",
        "**/activity-scope-label-screenshots.spec.ts",
        "**/welcome-agent-modal-screenshots.spec.ts",
        "**/local-archive-screenshots.spec.ts",
        "**/voice-settings.spec.ts",
        "**/agent-readiness-screenshots.spec.ts",
        "**/agent-error-state-screenshots.spec.ts",
        "**/edit-agent.spec.ts",
        "**/doctor-cta-screenshots.spec.ts",
        "**/pubkey-display-screenshots.spec.ts",
        "**/file-attachment.spec.ts",
        "**/image-attachment-gallery.spec.ts",
        "**/composer-image-draw.spec.ts",
        "**/video-attachment.spec.ts",
        "**/spoiler.spec.ts",
        "**/composer-link-shortcut.spec.ts",
        "**/entity-link-recipient-cards.spec.ts",
        "**/composer-selection-formatting.spec.ts",
        "**/composer-tooltip-dismiss.spec.ts",
        "**/mentions.spec.ts",
        "**/team-mentions.spec.ts",
        "**/persistent-agent-audience.spec.ts",
        "**/relay-reconnect.spec.ts",
        "**/relay-reconnect-affordance.spec.ts",
        "**/workflows.spec.ts",
        "**/identity-archive.spec.ts",
        "**/identity-archive-hide.spec.ts",
        "**/relay-connectivity.spec.ts",
        "**/unread-pill.spec.ts",
        "**/sidebar-more-unread-overlap.spec.ts",
        "**/sidebar-snapshot.spec.ts",
        "**/home-collapsed-top-chrome.spec.ts",
        "**/top-chrome-zoom-clearance.spec.ts",
        "**/thread-unread.spec.ts",
        "**/dm-new-message-screenshots.spec.ts",
        "**/signout-screenshots.spec.ts",
        "**/community-rail.spec.ts",
        "**/boot-splash.spec.ts",
        "**/thread-reply-anchor-roleplay.spec.ts",
        "**/threadpane-ultrawide.spec.ts",
        "**/thread-focus-mode.spec.ts",
        "**/animated-avatar.spec.ts",
        "**/reminders.spec.ts",
        "**/reminder-click-repro.spec.ts",
        "**/virtualization.spec.ts",
        "**/scroll-history.spec.ts",
        "**/channel-dense-second-reach.spec.ts",
        "**/channel-window-mock-paging.spec.ts",
        "**/live-broadcast-reply-timeline.spec.ts",
        "**/markdown-parse-cache.spec.ts",
        "**/overscroll-boundary.spec.ts",
        "**/terminal-wheel.spec.ts",
        "**/cold-switch-longtask.perf.ts",
        "**/timeline-no-shift.spec.ts",
        "**/human-edit-agent-content.spec.ts",
        "**/reaction-order.spec.ts",
        "**/reaction-names.spec.ts",
        "**/inbox-reactions.spec.ts",
        "**/inbox-edit.spec.ts",
        "**/send-channel-binding.spec.ts",
        "**/project-commit-detail.spec.ts",
        "**/project-inbox.spec.ts",
        "**/projects-v3-screenshots.spec.ts",
        "**/project-issue-comments.spec.ts",
        "**/project-pr-review.spec.ts",
        "**/projects-sidebar.spec.ts",
        "**/project-container-screen.spec.ts",
        "**/project-create-cold-start.spec.ts",
        "**/project-packs.spec.ts",
        "**/project-team-setup.spec.ts",
        // Tank Loop walkthrough: installed roles, lead picker, transport access.
        "**/project-roles-walkthrough.spec.ts",
        "**/project-repository-protection.spec.ts",
        "**/project-settings-screenshots.spec.ts",
        "**/projectPulse.spec.ts",
        "**/project-pulse-missions.spec.ts",
        "**/project-pulse-declared-work.spec.ts",
        "**/agentProgress.spec.ts",
        "**/persona-model-combobox-screenshots.spec.ts",
        "**/drafts-screenshots.spec.ts",
        "**/drafts-all-fix-screenshots.spec.ts",
        "**/inbox-refactor-screenshots.spec.ts",
        "**/buzz-theme-screenshots.spec.ts",
        "**/channel-sort.spec.ts",
        "**/identity-lost.spec.ts",
        "**/deep-link-invite.spec.ts",
        "**/invite-link-copy.spec.ts",
        "**/global-agent-config-screenshots.spec.ts",
        "**/doctor-states.spec.ts",
        "**/onboarding-avatar-skip.spec.ts",
        "**/onboarding-backup.spec.ts",
        "**/onboarding-agent-defaults.spec.ts",
        "**/mobile-pairing-qr.spec.ts",
        "**/profile-nsec-reveal.spec.ts",
        "**/profile-backup-settings.spec.ts",
        "**/signout-confirmation.spec.ts",
        "**/settings-section-layout.spec.ts",
        "**/experimental-features.spec.ts",
        "**/agent-provider-dropdowns.spec.ts",
        "**/agent-lifecycle-feedback.spec.ts",
        "**/agent-access-warning.spec.ts",
        "**/edit-agent-run-on.spec.ts",
        "**/inbox-live-update.spec.ts",
        // Ledger 238(e): the approval card the founder was never shown.
        "**/inbox-approval-request.spec.ts",
        "**/inbox-decision-request.spec.ts",
        "**/mesh-compute.spec.ts",
        "**/observer-archive-policy.spec.ts",
        "**/harness-management.spec.ts",
        "**/harness-catalog-screenshots.spec.ts",
        "**/inline-custom-harness.spec.ts",
        "**/where-to-run-config.spec.ts",
        "**/huddle-transcription.spec.ts",
        "**/agent-numeric-tuning.spec.ts",
        "**/mock-bridge-global-config-shape.spec.ts",
        "**/needs-restart-screenshots.spec.ts",
        // SV-36 S5 and SV-32 (session-view Wave C, C1).
        "**/coding-session-paragraph-streaming.spec.ts",
        "**/coding-session-file-chips.spec.ts",
        // SV-28 and SV-30 (session-view Wave C, C2).
        "**/coding-session-sv28-sv30-checkpoints.spec.ts",
        // SV-35 (session-view Wave C, C3a).
        "**/coding-session-sv35-model-switch.spec.ts",
      ],
      use: SMOKE_USE,
    },
    {
      // Smoke files that pass alone but fail beside three other workers — not
      // a reason to lower `workers` for everything. Run them as their own pass,
      // after `smoke` and with nothing else running (`pnpm test:e2e`,
      // `pnpm test:e2e:smoke` and `just e2e-affected` do, via
      // `scripts/e2e-passes.sh`): Playwright skips a project's dependents when
      // the dependency has any failure, so `dependencies` cannot order them.
      // A bare `playwright test` runs it beside the other projects. CI runs
      // one worker, so there it is serial anyway.
      // Evidence, 2026-10-04 at dad4726ba: empty-edit-delete failed 2 of 3 in
      // the four-worker full run and 1 of 12 alone at four, 0 of 21 at one;
      // nostr-bind :247 failed in the four-worker full run, 0 of 4 alone at
      // four and 0 of 7 at one.
      name: "smoke-serial",
      workers: 1,
      testMatch: ["**/empty-edit-delete.spec.ts", "**/nostr-bind.spec.ts"],
      use: SMOKE_USE,
    },
    {
      name: "integration",
      workers: 1,
      testMatch: [
        "**/agents.spec.ts",
        "**/agent-snapshot-recipient.spec.ts",
        "**/onboarding.spec.ts",
        "**/stream.spec.ts",
        "**/integration.spec.ts",
        "**/dm-double-notification.spec.ts",
        "**/profile.spec.ts",
        "**/sidebar.spec.ts",
        "**/sidebar-relay-card.spec.ts",
        "**/persona-env-vars.spec.ts",
        "**/persona-sync.spec.ts",
        "**/team-snapshot.spec.ts",
        "**/agents-everywhere.live.spec.ts",
        "**/relay-restart.live.spec.ts",
        "**/project-todos.live.spec.ts",
        "**/project-artifacts.live.spec.ts",
        "**/parity-ancestor-island.spec.ts",
      ],
      use: {
        ...devices["Desktop Chrome"],
      },
      expect: {
        timeout: process.env.CI ? 15_000 : 10_000,
      },
    },
    {
      // WebKit engine proxy, not WKWebView: Playwright's WebKit build on
      // macOS, not the webview the app ships in. Curated: boot plus the
      // coding-session specs (SV-37). No clipboard grant — WebKit has none.
      name: "smoke-webkit",
      testMatch: [
        "**/smoke.spec.ts",
        "**/coding-sessions.spec.ts",
        "**/coding-session-mission-lens.spec.ts",
        "**/coding-session-mission-density.spec.ts",
        "**/coding-session-observations.spec.ts",
        "**/coding-session-handover.spec.ts",
        "**/coding-session-founder-acts.spec.ts",
        "**/coding-session-reachability.spec.ts",
        "**/coding-session-model-picker.spec.ts",
        "**/coding-session-capacity.spec.ts",
        "**/coding-session-goal.spec.ts",
        "**/coding-session-founded-setup.spec.ts",
        "**/coding-session-auto-title.spec.ts",
        "**/coding-session-sv69-70-header.spec.ts",
        "**/coding-session-title-mode-settings.spec.ts",
        "**/coding-session-title-mode-founded.spec.ts",
        "**/coding-session-transcript-narrative-screenshots.spec.ts",
        "**/coding-session-project-boundary.spec.ts",
        "**/coding-session-parity-screenshots.spec.ts",
        "**/coding-session-umbrella-sandbox.spec.ts",
        "**/coding-session-wave-b-audit-markdown.spec.ts",
        "**/coding-session-wave-b-audit-transcript-model.spec.ts",
        "**/coding-session-wave-b-audit-transcript-view.spec.ts",
        "**/coding-session-subagent-rows.spec.ts",
        "**/coding-session-sv78-background-task.spec.ts",
        "**/coding-session-sv118-streaming.spec.ts",
        "**/coding-session-liveness.spec.ts",
        "**/coding-session-subagent-page.spec.ts",
        "**/coding-session-wave-b-audit-workspace.spec.ts",
        "**/coding-session-wave-b-followups.spec.ts",
        "**/coding-session-wave-b-registry.spec.ts",
        "**/coding-session-wave-b-header.spec.ts",
        "**/coding-session-wave-b-surfaces.spec.ts",
        "**/coding-session-wave-b-badges.spec.ts",
        "**/coding-session-wave-b-terminal.spec.ts",
        "**/coding-session-wave-b-minimap.spec.ts",
        "**/coding-session-wave-b-orchestration.spec.ts",
        "**/coding-session-terminal-shortcut.spec.ts",
        "**/coding-session-elision-screenshots.spec.ts",
        "**/coding-session-sv75-command-glyph.spec.ts",
        "**/coding-session-seat-bee.spec.ts",
        "**/coding-session-surface-host-screenshots.spec.ts",
        "**/coding-session-connect.spec.ts",
        "**/coding-session-native-steer.spec.ts",
        "**/coding-session-short-window.spec.ts",
        "**/coding-session-paging.spec.ts",
        "**/coding-session-width.spec.ts",
        "**/coding-session-worktree-source.spec.ts",
        "**/coding-session-workspace-reuse.spec.ts",
        "**/coding-session-worktree-closure.spec.ts",
        "**/coding-session-paragraph-streaming.spec.ts",
        "**/coding-session-file-chips.spec.ts",
        "**/coding-session-sv28-sv30-checkpoints.spec.ts",
        "**/coding-session-sv35-model-switch.spec.ts",
      ],
      use: { ...devices["Desktop Safari"] },
    },
  ],
  webServer: {
    // Not `python3 -m http.server`: its five-deep listen backlog resets the
    // surplus of a ~530-chunk cold load, and the route under test never
    // mounts. See `scripts/e2e-preview-server.py`.
    command: `python3 scripts/e2e-preview-server.py --port ${PREVIEW_PORT} --directory dist`,
    cwd: ".",
    reuseExistingServer: !process.env.CI,
    url: PREVIEW_URL,
  },
});
