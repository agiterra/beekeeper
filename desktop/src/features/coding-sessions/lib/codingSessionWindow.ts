import {
  buildCodingSessionPopoutUrl,
  buildCodingSessionWindowLabel,
} from "./codingSessionRoute";
import {
  getCodingSessionPopoutBootstrap,
  type CodingSessionPopoutBootstrap,
} from "./codingSessionBootstrap";

type CodingSessionWindowHandle = {
  label: string;
  setFocus(): Promise<void>;
  show(): Promise<void>;
  unminimize(): Promise<void>;
};

type PendingCodingSessionWindowHandle = CodingSessionWindowHandle & {
  once(
    event: "tauri://created" | "tauri://error",
    handler: (event: { payload: unknown }) => void,
  ): Promise<() => void>;
};

export type CodingSessionWindowApi = {
  getByLabel(label: string): Promise<CodingSessionWindowHandle | null>;
  stageBootstrap?(
    label: string,
    bootstrap: CodingSessionPopoutBootstrap,
  ): Promise<void>;
  create(
    label: string,
    options: {
      title: string;
      url: string;
      width: number;
      height: number;
      minWidth: number;
      minHeight: number;
      center: boolean;
      focus: boolean;
      visible: boolean;
    },
  ): PendingCodingSessionWindowHandle;
};

async function getDefaultWindowApi(): Promise<CodingSessionWindowApi> {
  const { WebviewWindow } = await import("@tauri-apps/api/webviewWindow");
  const { invoke } = await import("@tauri-apps/api/core");
  return {
    getByLabel: (label) => WebviewWindow.getByLabel(label),
    stageBootstrap: (label, bootstrap) =>
      invoke("stage_coding_session_popout_bootstrap", { label, bootstrap }),
    create: (label, options) => new WebviewWindow(label, options),
  };
}

async function focusWindow(window: CodingSessionWindowHandle): Promise<void> {
  await window.unminimize();
  await window.show();
  await window.setFocus();
}

async function waitForCreatedWindow(
  window: PendingCodingSessionWindowHandle,
): Promise<void> {
  await new Promise<void>((resolve, reject) => {
    void window.once("tauri://created", () => resolve());
    void window.once("tauri://error", (event) => {
      reject(
        new Error(
          typeof event.payload === "string"
            ? event.payload
            : "Failed to create coding-session window.",
        ),
      );
    });
  });
}

/** Focus the deterministic pop-out for an exact route, or create it once. */
export async function openCodingSessionPopout(
  channelId: string,
  generationId: string,
  api?: CodingSessionWindowApi,
): Promise<{ created: boolean; label: string }> {
  const windowApi = api ?? (await getDefaultWindowApi());
  const label = buildCodingSessionWindowLabel(channelId, generationId);
  const existing = await windowApi.getByLabel(label);
  if (existing) {
    await focusWindow(existing);
    return { created: false, label };
  }

  const bootstrap = getCodingSessionPopoutBootstrap(channelId, generationId);
  if (!bootstrap && windowApi.stageBootstrap) {
    throw new Error(
      "Exact signed session snapshot is not ready yet. Reopen this generation from the current catalog.",
    );
  }
  if (bootstrap && windowApi.stageBootstrap) {
    await windowApi.stageBootstrap(label, bootstrap);
  }

  const window = windowApi.create(label, {
    title: "Coding session",
    url: buildCodingSessionPopoutUrl(channelId, generationId),
    width: 1_080,
    height: 820,
    minWidth: 680,
    minHeight: 500,
    center: true,
    focus: true,
    visible: true,
  });
  try {
    await waitForCreatedWindow(window);
    await focusWindow(window);
    return { created: true, label };
  } catch (error) {
    // A simultaneous click can win creation between getByLabel and create.
    const racedWindow = await windowApi.getByLabel(label);
    if (!racedWindow) throw error;
    await focusWindow(racedWindow);
    return { created: false, label };
  }
}
