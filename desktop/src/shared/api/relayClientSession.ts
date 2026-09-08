import { Channel, invoke } from "@tauri-apps/api/core";
import { createAuthEvent, getRelayWsUrl } from "@/shared/api/tauri";
import { queryRelayFilters } from "@/shared/api/relayQueryBridge";
import type { RelayEvent } from "@/shared/api/types";
import {
  getTextPayload,
  MAX_FILTERS_PER_REQ,
  type ConnectionState,
  type LiveSubscriptionReadiness,
  type PendingEvent,
  type RelaySubscription,
  type RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import {
  clearClosedRetry,
  handleRelayClosed,
  handleSubscriptionEose,
  prepareSubscriptionEvent,
} from "@/shared/api/relayClosedRecovery";
import { replayLiveSubscriptions } from "@/shared/api/relayReconnectReplay";
import { publishSessionEvent } from "@/shared/api/relayEventPublisher";
import {
  activateRateLimitIfSignalled,
  waitForRateLimit,
} from "@/shared/api/relayRateLimitGate";
import { requestHistoryGated } from "@/shared/api/relayGateBoundary";
import {
  executeQueryBatch,
  RelayQueryCoalescer,
  unionQueryResults,
} from "@/shared/api/relayQueryCoalescer";
import { admitFrame } from "@/shared/api/relaySendBudget";
import { RelaySubscriptionRegistry } from "@/shared/api/relaySubscriptionRegistry";
import { RelayConnectionStateEmitter } from "@/shared/api/relayConnectionStateEmitter";
import {
  isServiceRestartClose,
  isWebSocketClose,
  isWebSocketError,
  jitteredReconnectDelayMs,
  shouldRefuseConnect,
  shouldScheduleReconnect,
  shouldWaitForScheduledReconnect,
} from "@/shared/api/relayReconnectPolicy";
import { RelayReconnectWaiters } from "@/shared/api/relayReconnectWaiters";
import { RelayStallWatchdog } from "@/shared/api/relayStallWatchdog";
import {
  AUTH_TIMEOUT_MS,
  BACKOFF_RESET_STABLE_MS,
  EVENT_BATCH_MS,
  HISTORY_TIMEOUT_MS,
  RECONNECT_BASE_DELAY_MS,
  RECONNECT_MAX_DELAY_MS,
  STALL_CHECK_INTERVAL_MS,
  STALL_IDLE_TIMEOUT_MS,
} from "@/shared/api/relayClientTimings";
import { closeWebSocket } from "@/shared/api/relayWebSocketClose";
import { AuthOkTracker } from "@/shared/api/relayAuthPolicy";

/**
 * The product-facing client (`RelayClient`) lives in
 * `relayClientFeatureApi.ts`; this file is the transport core it extends.
 * Re-exported as a type so feature code that names the client type keeps
 * one import path.
 */
export type { RelayClient } from "@/shared/api/relayClientFeatureApi";

/**
 * Relay WebSocket session: connect + NIP-42 AUTH, live subscriptions,
 * publishes, reconnect and replay. Feature-level wrappers (channel history,
 * typing, presence, …) are added by `RelayClient` in
 * `relayClientFeatureApi.ts`, which is why the members below are `protected`
 * rather than `private`.
 */
export class RelaySessionCore {
  protected wsId: number | null = null;
  private relayUrl: string | null = null;
  private connectPromise: Promise<number> | null = null;
  private reconnectTimeout: number | null = null;
  private reconnectWaiters = new RelayReconnectWaiters();
  private reconnectDelayMs = RECONNECT_BASE_DELAY_MS;
  private keepAliveRequested = false;
  private authRequest: {
    pendingEventId: string;
    resolve: () => void;
    reject: (error: Error) => void;
    timeout: number;
  } | null = null;
  protected subscriptions = new Map<string, RelaySubscription>();
  private pendingEvents = new Map<string, PendingEvent>();
  private eventBuffer: Array<{ subId: string; event: RelayEvent }> = [];
  private flushTimeout: number | null = null;
  private reconnectListeners = new Set<() => void>();
  private hasConnectedOnce = false;
  private notifyReconnectListeners = false;
  private onMessageChannel: Channel<unknown> | null = null;
  private connectionGeneration = 0;
  private sessionEpoch = 0;
  private stabilityTimer: number | null = null;
  private visibleChannelId: string | null = null;
  private authOkTracker = new AuthOkTracker();
  private subscriptionRegistry = new RelaySubscriptionRegistry();
  /**
   * One-shot reads issued within 50 ms share one `POST /query`; an HTTP
   * failure sends each filter down the WS history path on its own.
   */
  protected queryCoalescer = new RelayQueryCoalescer({
    execute: (filters) =>
      executeQueryBatch(filters, {
        query: queryRelayFilters,
        fallback: (filter) => this.fetchHistory(filter),
      }),
  });

  private terminal = false;

  private connectionStateEmitter = new RelayConnectionStateEmitter("idle");
  private stallWatchdog = new RelayStallWatchdog({
    intervalMs: STALL_CHECK_INTERVAL_MS,
    idleTimeoutMs: STALL_IDLE_TIMEOUT_MS,
    onStall: (error) => {
      this.connectionStateEmitter.set("stalled");
      this.resetConnection(error);
    },
  });

  setVisibleChannelId(id: string | null) {
    this.visibleChannelId = id;
  }

  disconnect() {
    const error = new Error("Relay disconnected for community switch.");

    if (this.reconnectTimeout) {
      window.clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    if (this.stabilityTimer !== null) {
      window.clearTimeout(this.stabilityTimer);
      this.stabilityTimer = null;
    }
    this.stallWatchdog.stop();
    this.sessionEpoch++;
    this.connectionGeneration++;
    this.keepAliveRequested = false;
    this.relayUrl = null;
    this.hasConnectedOnce = false;
    this.notifyReconnectListeners = false;
    this.terminal = false;
    this.visibleChannelId = null;
    this.authOkTracker.reset();
    this.connectionStateEmitter.set("idle");

    if (this.wsId !== null) {
      void closeWebSocket(this.wsId, "community switch");
      this.wsId = null;
    }

    this.connectPromise = null;
    this.reconnectWaiters.settle(error);

    if (this.authRequest) {
      window.clearTimeout(this.authRequest.timeout);
      this.authRequest.reject(error);
      this.authRequest = null;
    }

    for (const [subId, sub] of this.subscriptions) {
      if (sub.mode !== "live") {
        window.clearTimeout(sub.timeout);
        sub.reject(error);
      } else {
        clearClosedRetry(sub);
      }
      this.subscriptions.delete(subId);
    }
    // The live entries are gone with the socket; a later join for the same
    // filters must open a fresh REQ rather than attach to a dead entry.
    this.subscriptionRegistry.clear();
    this.queryCoalescer.reset(error);

    for (const [eventId, pending] of this.pendingEvents) {
      window.clearTimeout(pending.timeout);
      pending.reject(error);
      this.pendingEvents.delete(eventId);
    }

    if (this.flushTimeout !== null) {
      window.clearTimeout(this.flushTimeout);
      this.flushTimeout = null;
    }
    this.eventBuffer = [];
    this.reconnectListeners.clear();
    this.connectionStateEmitter.clear();
    this.onMessageChannel = null;
    this.reconnectDelayMs = RECONNECT_BASE_DELAY_MS;
  }

  protected async fetchHistory(filter: RelaySubscriptionFilter) {
    await this.ensureConnected();
    return this.requestHistory(filter);
  }

  protected requestHistory(
    filter: RelaySubscriptionFilter,
  ): Promise<RelayEvent[]> {
    return requestHistoryGated(
      this.subscriptions,
      (payload) => this.sendRaw(payload),
      (subId) => this.closeSubscription(subId),
      filter,
      HISTORY_TIMEOUT_MS,
    );
  }

  /**
   * Batched one-shot read over `POST /query` with per-filter WS fallback,
   * without the connect gate — safe to call from inside `connect()` (replay),
   * where `ensureConnected()` would wait on itself.
   */
  protected async requestHistoryBatch(
    filters: RelaySubscriptionFilter[],
  ): Promise<RelayEvent[]> {
    return unionQueryResults(
      await executeQueryBatch(filters, {
        query: queryRelayFilters,
        fallback: (filter) => this.requestHistory(filter),
      }),
    );
  }

  async preconnect() {
    // Explicit re-engagement (reconnect card / community switch): clears the
    // terminal latch and AUTH rejection streak, and bypasses backoff once.
    this.terminal = false;
    this.authOkTracker.reset();
    this.keepAliveRequested = true;
    await this.connectBypassingBackoff();
  }

  /**
   * Environment-driven resume (online/focus/visibility): bypasses a pending
   * backoff timer but preserves the terminal latch and AUTH rejection streak
   * — only `preconnect()` clears those, so resume events during repeated
   * AUTH rejection cannot defeat the consecutive-rejection cap.
   */
  async resumeReconnect() {
    if (this.terminal) return;
    await this.connectBypassingBackoff();
  }

  private async connectBypassingBackoff() {
    if (this.reconnectTimeout !== null) {
      window.clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    try {
      await this.ensureConnected();
      this.reconnectWaiters.settle();
    } catch (error) {
      this.reconnectWaiters.settle(
        this.normalizeRelayError(error, "Relay reconnect failed."),
      );
      throw error;
    }
  }

  subscribeToReconnects(listener: () => void) {
    this.reconnectListeners.add(listener);
    return () => {
      this.reconnectListeners.delete(listener);
    };
  }

  /** Current connection state — synchronous read. */
  getConnectionState(): ConnectionState {
    return this.connectionStateEmitter.get();
  }

  /**
   * Subscribe to connection-state transitions. The listener fires
   * immediately with the current state, so callers need no separate
   * `getConnectionState()` call to seed their UI.
   */
  subscribeToConnectionState(listener: (state: ConnectionState) => void) {
    return this.connectionStateEmitter.subscribe(listener);
  }

  protected async ensureConnected() {
    if (shouldRefuseConnect({ terminal: this.terminal })) {
      // Terminal (e.g. relay rejected auth): refuse until disconnect() or
      // preconnect() clears the latch, else the reconnect-timer catch and
      // the publish/subscribe retry wrappers would race the terminal
      // "disconnected" state back to "reconnecting".
      throw new Error("Relay session is terminal; cannot reconnect.");
    }

    if (this.connectPromise) {
      return this.connectPromise;
    }

    if (this.wsId !== null) {
      return this.connectionGeneration;
    }

    if (
      shouldWaitForScheduledReconnect({
        hasPendingReconnect: this.reconnectTimeout !== null,
      })
    ) {
      // The reconnect coordinator owns outage pacing. Query, publish, and
      // subscription callers must wait for its scheduled attempt instead of
      // clearing the timer and creating an immediate reconnect storm.
      return this.reconnectWaiters.wait().then(() => this.connectionGeneration);
    }

    const connectPromise = this.connect();
    this.connectPromise = connectPromise;

    try {
      return await connectPromise;
    } finally {
      if (this.connectPromise === connectPromise) {
        this.connectPromise = null;
      }
    }
  }

  private async connect() {
    if (this.stabilityTimer !== null) {
      window.clearTimeout(this.stabilityTimer);
      this.stabilityTimer = null;
    }

    this.connectionStateEmitter.set(
      this.hasConnectedOnce ? "reconnecting" : "connecting",
    );

    const generation = ++this.connectionGeneration;
    this.onMessageChannel = new Channel<unknown>((message) => {
      void this.handleWsMessage(message, generation).catch((error) => {
        if (generation !== this.connectionGeneration) return;
        this.resetConnection(
          this.normalizeRelayError(error, "Relay connection errored."),
        );
      });
    });

    try {
      if (!this.relayUrl) {
        this.relayUrl = await getRelayWsUrl();
      }
      const wsId = await invoke<number>("plugin:websocket|connect", {
        url: this.relayUrl,
        onMessage: this.onMessageChannel,
        config: {},
      });
      if (generation !== this.connectionGeneration) {
        void closeWebSocket(wsId, "stale connection attempt");
        throw new Error("Relay connection attempt was superseded.");
      }
      this.wsId = wsId;

      await new Promise<void>((resolve, reject) => {
        const timeout = window.setTimeout(() => {
          const error = new Error("Relay authentication timed out.");
          this.authRequest = null;
          this.resetConnection(error);
          reject(error);
        }, AUTH_TIMEOUT_MS);

        this.authRequest = {
          pendingEventId: "",
          resolve,
          reject,
          timeout,
        };
      });

      this.stabilityTimer = window.setTimeout(() => {
        this.stabilityTimer = null;
        this.reconnectDelayMs = RECONNECT_BASE_DELAY_MS;
      }, BACKOFF_RESET_STABLE_MS);

      this.connectionStateEmitter.set("connected");
      await this.replayLiveSubscriptions();
      this.stallWatchdog.start();
      this.emitReconnectIfNeeded();
      return generation;
    } catch (error) {
      const connectionError = this.normalizeRelayError(
        error,
        "Failed to connect to relay.",
      );
      if (generation === this.connectionGeneration) {
        this.resetConnection(connectionError);
      }
      throw connectionError;
    }
  }

  protected subscribe(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
    onReady?: (readiness: LiveSubscriptionReadiness) => void,
    readinessTimeoutMs = 250,
  ) {
    return this.subscribeMany([filter], onEvent, onReady, readinessTimeoutMs);
  }

  /**
   * Live subscription over one REQ carrying `filters` (1–10, OR-ed by the
   * relay). Identical filter lists share one REQ through the registry; the
   * returned function leaves it and sends CLOSE once nobody holds it.
   */
  protected subscribeMany(
    filters: RelaySubscriptionFilter[],
    onEvent: (event: RelayEvent) => void,
    onReady?: (readiness: LiveSubscriptionReadiness) => void,
    readinessTimeoutMs = 250,
  ): Promise<() => Promise<void>> {
    if (filters.length === 0 || filters.length > MAX_FILTERS_PER_REQ) {
      return Promise.reject(
        new Error(
          `A relay REQ carries 1 to ${MAX_FILTERS_PER_REQ} filters, got ${filters.length}.`,
        ),
      );
    }
    return this.subscriptionRegistry.join(
      filters,
      onEvent,
      onReady,
      (sharedFilters, fanOut, ready) =>
        this.openLiveSubscription(
          sharedFilters,
          fanOut,
          ready,
          readinessTimeoutMs,
        ),
    );
  }

  private async openLiveSubscription(
    filters: RelaySubscriptionFilter[],
    onEvent: (event: RelayEvent) => void,
    onReady: (readiness: LiveSubscriptionReadiness) => void,
    readinessTimeoutMs: number,
  ) {
    await this.ensureConnected();
    // Back-pressure already signalled: a REQ now would be refused and cost
    // an admission unit for nothing.
    await waitForRateLimit();

    const subId = `live-${crypto.randomUUID()}`;
    let resolveReady = (_readiness: LiveSubscriptionReadiness) => {};
    const ready = new Promise<void>((resolve) => {
      resolveReady = (readiness) => {
        window.clearTimeout(fallbackTimeout);
        onReady(readiness);
        resolve();
      };
    });
    const fallbackTimeout = window.setTimeout(
      () => resolveReady("timeout"),
      readinessTimeoutMs,
    );

    this.subscriptions.set(subId, {
      mode: "live",
      filters,
      onEvent,
      resolveReady,
    });

    try {
      await this.sendRawWithReconnectRetry(
        ["REQ", subId, ...filters],
        "Failed to restore relay subscription.",
      );
    } catch (error) {
      window.clearTimeout(fallbackTimeout);
      this.subscriptions.delete(subId);
      throw error;
    }
    await ready;

    return async () => {
      const active = this.subscriptions.get(subId);
      if (active?.mode !== "live") {
        return;
      }

      this.subscriptions.delete(subId);
      clearClosedRetry(active);
      await this.closeSubscription(subId);
    };
  }

  /**
   * Send one client frame, charging the send budget for its lane first
   * (`relaySendBudget.ts`). A caller that already holds a slot — the typing
   * indicator's `tryAcquire` — passes `preAdmitted` so the frame is not
   * charged twice.
   */
  protected async sendRaw(
    payload: unknown[],
    options?: { preAdmitted?: boolean },
  ) {
    if (this.wsId === null) {
      throw new Error("Relay socket is not connected.");
    }
    if (!options?.preAdmitted) {
      const generation = this.connectionGeneration;
      const admission = admitFrame(
        payload,
        () => generation === this.connectionGeneration && this.wsId !== null,
        "Relay send was superseded by a session change.",
      );
      if (admission) await admission;
    }

    await invoke("plugin:websocket|send", {
      id: this.wsId,
      message: {
        type: "Text",
        data: JSON.stringify(payload),
      },
    });
  }

  private async sendRawForGeneration(payload: unknown[], generation: number) {
    if (generation !== this.connectionGeneration || this.wsId === null) {
      throw new Error("Relay publish was superseded by a session change.");
    }
    const admission = admitFrame(
      payload,
      () => generation === this.connectionGeneration && this.wsId !== null,
      "Relay publish was superseded by a session change.",
    );
    if (admission) await admission;
    const wsId = this.wsId;
    await invoke("plugin:websocket|send", {
      id: wsId,
      message: { type: "Text", data: JSON.stringify(payload) },
    });
  }

  private normalizeRelayError(error: unknown, fallbackMessage: string) {
    return error instanceof Error ? error : new Error(fallbackMessage);
  }

  private recoverFromSocketFailure(
    error: unknown,
    fallbackMessage: string,
  ): Error {
    const normalizedError = this.normalizeRelayError(error, fallbackMessage);
    this.resetConnection(normalizedError);
    return normalizedError;
  }

  private async sendRawWithReconnectRetry(
    payload: unknown[],
    fallbackMessage: string,
  ) {
    const generation = this.connectionGeneration;
    try {
      await this.sendRaw(payload);
    } catch (error) {
      // A send that lost its socket while waiting for a budget slot failed
      // against a connection that is already gone; resetting again would
      // tear down its replacement.
      const normalizedError =
        generation === this.connectionGeneration
          ? this.recoverFromSocketFailure(error, fallbackMessage)
          : this.normalizeRelayError(error, fallbackMessage);
      try {
        await this.ensureConnected();
        await this.sendRaw(payload);
      } catch (retryError) {
        throw this.recoverFromSocketFailure(
          retryError,
          normalizedError.message,
        );
      }
    }
  }

  protected async closeSubscription(subId: string) {
    if (this.wsId === null) {
      return;
    }

    await this.sendRaw(["CLOSE", subId]);
  }

  async publishEvent(
    event: RelayEvent,
    timeoutMessage: string,
    sendErrorMessage: string,
  ) {
    return publishSessionEvent(
      {
        generation: () => this.connectionGeneration,
        ownership: () => this.sessionEpoch,
        pendingEvents: this.pendingEvents,
        send: (payload, generation) =>
          this.sendRawForGeneration(payload, generation),
        reconnect: () => this.ensureConnected(),
        normalizeError: (error, fallback) =>
          this.normalizeRelayError(error, fallback),
        recoverSocketFailure: (error, fallback) =>
          this.recoverFromSocketFailure(error, fallback),
      },
      event,
      timeoutMessage,
      sendErrorMessage,
    );
  }

  private async handleWsMessage(message: unknown, generation: number) {
    if (generation !== this.connectionGeneration) return;
    this.stallWatchdog.recordInbound();

    if (isWebSocketClose(message)) {
      if (isServiceRestartClose(message))
        this.reconnectDelayMs = RECONNECT_BASE_DELAY_MS;
      this.resetConnection(new Error("Relay connection closed."));
      return;
    }
    if (isWebSocketError(message)) {
      this.resetConnection(new Error("Relay connection errored."));
      return;
    }

    const payload = getTextPayload(message);
    if (!payload) {
      return;
    }

    let data: unknown;
    try {
      data = JSON.parse(payload);
    } catch {
      return;
    }

    if (!Array.isArray(data) || data.length === 0) {
      return;
    }

    const [type, ...rest] = data;
    if (type === "AUTH" && typeof rest[0] === "string") {
      await this.handleAuthChallenge(rest[0], generation);
      return;
    }
    if (type === "EVENT" && typeof rest[0] === "string" && rest[1]) {
      this.handleEvent(rest[0], rest[1] as RelayEvent);
      return;
    }

    if (
      type === "OK" &&
      typeof rest[0] === "string" &&
      typeof rest[1] === "boolean"
    ) {
      this.handleOk(
        rest[0],
        rest[1],
        typeof rest[2] === "string" ? rest[2] : "",
      );
      return;
    }

    if (type === "EOSE" && typeof rest[0] === "string") {
      this.handleEose(rest[0]);
      return;
    }

    if (type === "CLOSED" && typeof rest[0] === "string") {
      handleRelayClosed({
        subscriptions: this.subscriptions,
        subId: rest[0],
        message: typeof rest[1] === "string" ? rest[1] : "",
        sendReq: (subId, filters) =>
          this.sendRawWithReconnectRetry(
            ["REQ", subId, ...filters],
            "Failed to restore relay subscription after CLOSED.",
          ),
      });
      return;
    }

    if (type === "NOTICE" && typeof rest[0] === "string") {
      // Connection-scoped back-pressure — arm the gate until it expires.
      activateRateLimitIfSignalled(rest[0]);
    }
  }

  private async handleAuthChallenge(challenge: string, generation: number) {
    if (!this.relayUrl) {
      this.relayUrl = await getRelayWsUrl();
    }

    const event = await createAuthEvent({
      challenge,
      relayUrl: this.relayUrl,
    });

    if (generation !== this.connectionGeneration || !this.authRequest) {
      return;
    }

    this.authRequest.pendingEventId = event.id;
    await this.sendRaw(["AUTH", event]);
  }

  private handleEvent(subId: string, event: RelayEvent) {
    const subscription = this.subscriptions.get(subId);
    if (!subscription) {
      return;
    }

    if (subscription.mode === "first") {
      subscription.onEvent(event);
      return;
    }

    if (!prepareSubscriptionEvent(subscription, event)) return;
    this.eventBuffer.push({ subId, event });
    this.flushTimeout ??= window.setTimeout(
      () => this.flushEventBuffer(),
      EVENT_BATCH_MS,
    );
  }

  private flushEventBuffer() {
    this.flushTimeout = null;
    const buffer = this.eventBuffer;
    this.eventBuffer = [];

    // Re-lookup: subscriptions removed during batch window are intentionally skipped.
    for (const { subId, event } of buffer) {
      const subscription = this.subscriptions.get(subId);
      if (subscription?.mode === "live") {
        subscription.onEvent(event);
      }
    }
  }

  private handleEose(subId: string) {
    this.flushEventBuffer(); // Deliver preceding EVENT frames before EOSE.
    handleSubscriptionEose({
      subscriptions: this.subscriptions,
      subId,
      closeSubscription: (id) => this.closeSubscription(id),
    });
  }

  private handleOk(eventId: string, success: boolean, message: string) {
    if (this.authRequest && this.authRequest.pendingEventId === eventId) {
      window.clearTimeout(this.authRequest.timeout);
      const authRequest = this.authRequest;
      this.authRequest = null;

      // Decision table lives in relayAuthPolicy.ts.
      const decision = this.authOkTracker.record(success, message);
      if (decision === "authenticated") {
        authRequest.resolve();
      } else {
        const error = new Error(message || "Relay authentication rejected.");
        authRequest.reject(error);
        this.resetConnection(error, { reconnect: decision === "retry" });
      }

      return;
    }

    // Back-pressure now arrives here rather than as a NOTICE: the relay
    // rejects an over-quota EVENT on the OK channel. Arm the gate before the
    // pending lookup — a refusal addressed to an event this session no
    // longer tracks (a fire-and-forget typing frame, a retried publish) is
    // still the relay telling us to back off.
    if (!success) activateRateLimitIfSignalled(message);

    const pendingEvent = this.pendingEvents.get(eventId);
    if (!pendingEvent) {
      return;
    }

    window.clearTimeout(pendingEvent.timeout);
    this.pendingEvents.delete(eventId);

    if (success) {
      pendingEvent.resolve(pendingEvent.event);
    } else {
      pendingEvent.reject(new Error(message || "Relay rejected the event."));
    }
  }

  private hasLiveSubscriptions() {
    return [...this.subscriptions.values()].some((s) => s.mode === "live");
  }

  private async replayLiveSubscriptions() {
    const generation = this.connectionGeneration;
    try {
      await replayLiveSubscriptions({
        subscriptions: this.subscriptions,
        sendRaw: (payload) => this.sendRaw(payload),
        requestHistoryBatch: (filters) => this.requestHistoryBatch(filters),
        visibleChannelId: this.visibleChannelId,
        isActive: () => this.connectionGeneration === generation,
      });
    } catch (error) {
      const reconnectError =
        error instanceof Error
          ? error
          : new Error("Failed to restore relay subscriptions.");
      this.resetConnection(reconnectError);
      throw reconnectError;
    }
  }

  private scheduleReconnect() {
    if (
      !shouldScheduleReconnect({
        terminal: this.terminal,
        hasPendingReconnect: this.reconnectTimeout !== null,
        hasLiveSocket: this.wsId !== null,
        keepAliveRequested: this.keepAliveRequested,
        hasLiveSubscriptions: this.hasLiveSubscriptions(),
      })
    ) {
      return;
    }

    const delay = jitteredReconnectDelayMs(
      this.reconnectDelayMs,
      RECONNECT_MAX_DELAY_MS,
    );
    this.reconnectDelayMs = Math.min(
      this.reconnectDelayMs * 2,
      RECONNECT_MAX_DELAY_MS,
    );

    this.reconnectTimeout = window.setTimeout(() => {
      this.reconnectTimeout = null;
      void this.ensureConnected()
        .then(() => this.reconnectWaiters.settle())
        .catch((error) => {
          this.reconnectWaiters.settle(
            this.normalizeRelayError(error, "Relay reconnect failed."),
          );
          this.scheduleReconnect();
        });
    }, delay);
  }

  private emitReconnectIfNeeded() {
    const shouldNotifyReconnectListeners =
      this.hasConnectedOnce && this.notifyReconnectListeners;

    this.hasConnectedOnce = true;
    this.notifyReconnectListeners = false;

    if (!shouldNotifyReconnectListeners) {
      return;
    }

    for (const listener of this.reconnectListeners) {
      try {
        listener();
      } catch (error) {
        console.error("Failed to handle relay reconnect", error);
      }
    }
  }

  private resetConnection(
    error: Error,
    options?: {
      reconnect?: boolean;
    },
  ) {
    this.onMessageChannel = null;
    this.stallWatchdog.stop();
    this.connectionGeneration++;
    if (this.stabilityTimer !== null) {
      window.clearTimeout(this.stabilityTimer);
      this.stabilityTimer = null;
    }
    if (this.flushTimeout !== null) window.clearTimeout(this.flushTimeout);
    this.flushTimeout = null;
    this.eventBuffer = [];

    if (options?.reconnect === false) {
      this.terminal = true;
      this.connectionStateEmitter.set("disconnected");
    } else if (
      // A late retry failure racing a terminal latch must not paint
      // "reconnecting" over the terminal "disconnected" state; stall is a
      // stronger signal than a generic drop and is kept until reconnect.
      !this.terminal &&
      this.connectionStateEmitter.get() !== "stalled"
    ) {
      this.connectionStateEmitter.set("reconnecting");
    }

    if (options?.reconnect !== false && this.hasConnectedOnce) {
      this.notifyReconnectListeners = true;
    }

    if (options?.reconnect === false && this.reconnectTimeout) {
      window.clearTimeout(this.reconnectTimeout);
      this.reconnectTimeout = null;
    }
    if (options?.reconnect === false) {
      this.reconnectWaiters.settle(error);
    }

    if (this.wsId !== null) {
      void closeWebSocket(this.wsId, "connection reset");
    }

    this.wsId = null;

    if (this.authRequest) {
      window.clearTimeout(this.authRequest.timeout);
      this.authRequest.reject(error);
      this.authRequest = null;
    }

    for (const [subId, subscription] of this.subscriptions) {
      if (subscription.mode !== "live") {
        window.clearTimeout(subscription.timeout);
        subscription.reject(error);
        this.subscriptions.delete(subId);
        continue;
      }
      subscription.resolveReady?.("closed");
      subscription.resolveReady = undefined;
      clearClosedRetry(subscription);
    }
    for (const [eventId, pendingEvent] of this.pendingEvents) {
      window.clearTimeout(pendingEvent.timeout);
      pendingEvent.reject(error);
      this.pendingEvents.delete(eventId);
    }
    if (options?.reconnect !== false) {
      this.scheduleReconnect();
    }
  }
}
