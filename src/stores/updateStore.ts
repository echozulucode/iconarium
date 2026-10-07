import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import { isTauri } from "../api/backend";

/**
 * Auto-update state and the flow that drives it (same design as Richochet's updater).
 *
 * ```text
 * idle → checking → up-to-date | available → downloading → ready-to-install
 *                      ↓ (any step)
 *                    error
 * ```
 *
 * Everything is best-effort: no network, a GitHub outage or a proxy eating the request must never
 * stop the app from working, so every failure lands in `error`, which renders the same quiet
 * version line as `idle`. The next launch checks again.
 */
export type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "up-to-date" }
  | { kind: "available"; version: string; notes: string | null }
  /** `percent` is null when the server sent no content length. */
  | { kind: "downloading"; version: string; percent: number | null }
  | { kind: "ready-to-install"; version: string }
  | { kind: "error" };

export type UpdateDownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

export interface UpdateHandle {
  version: string;
  notes: string | null;
  /** Fetch the package; resolves when the bytes are on disk, before anything is installed. */
  download: (onEvent: (event: UpdateDownloadEvent) => void) => Promise<void>;
  /**
   * Install the downloaded package. On Windows this never returns: the plugin starts the NSIS
   * installer (passive mode) and exits the process. That is why download and install are separate
   * steps: a combined call would quit the moment the bytes landed.
   */
  install: () => Promise<void>;
}

/**
 * Seam between the store and Tauri. The plugins are loaded with dynamic `import()` so neither
 * Vitest (jsdom) nor the browser mock UI ever loads them; outside Tauri the client is `null` and
 * the store simply stays idle.
 */
export interface UpdaterClient {
  getVersion: () => Promise<string>;
  check: () => Promise<UpdateHandle | null>;
  relaunch: () => Promise<void>;
}

const tauriClient: UpdaterClient = {
  async getVersion() {
    const { getVersion } = await import("@tauri-apps/api/app");
    return getVersion();
  },
  async check() {
    const { check } = await import("@tauri-apps/plugin-updater");
    const update = await check();
    if (!update) return null;
    return {
      version: update.version,
      notes: update.body ?? null,
      download: (onEvent) => update.download((event) => onEvent(event as UpdateDownloadEvent)),
      install: () => update.install(),
    };
  },
  async relaunch() {
    const { relaunch } = await import("@tauri-apps/plugin-process");
    await relaunch();
  },
};

let activeClient: UpdaterClient | null = isTauri() ? tauriClient : null;

/** Test seam: swap in a fake client (or `null` to simulate running outside Tauri). */
export function __setUpdaterClientForTests(client: UpdaterClient | null): void {
  activeClient = client;
}

export interface UpdateStore {
  state: UpdateState;
  /** Running version, or null when unknown (browser/mock mode). */
  currentVersion: string | null;
  hasChecked: boolean;
  /** Read the running version and ask for updates. Never rejects. */
  checkForUpdates: () => Promise<void>;
  /** `checkForUpdates`, at most once per session. Never rejects. */
  checkOnLaunch: () => Promise<void>;
  /** Download the available package. Never rejects. */
  startDownload: () => Promise<void>;
  /** Install what was downloaded and restart into it. Never rejects. */
  restart: () => Promise<void>;
}

function warn(step: string, error: unknown): void {
  const message = error instanceof Error ? error.message : String(error);
  console.warn(`[update] ${step} failed: ${message}`);
}

export function createUpdateStore(): StoreApi<UpdateStore> {
  let handle: UpdateHandle | null = null;
  let restarting = false;

  return createStore<UpdateStore>()((set, get) => ({
    state: { kind: "idle" },
    currentVersion: null,
    hasChecked: false,

    async checkForUpdates() {
      const client = activeClient;
      if (!client || get().state.kind === "checking") return;
      const busy = get().state.kind;
      if (busy === "downloading" || busy === "ready-to-install") return;
      set({ state: { kind: "checking" }, hasChecked: true });

      // The version is only a label; its failure is not the update flow's failure.
      const version = client
        .getVersion()
        .then((v) => set({ currentVersion: v }))
        .catch(() => undefined);

      try {
        const found = await client.check();
        handle = found;
        set(
          found
            ? { state: { kind: "available", version: found.version, notes: found.notes } }
            : { state: { kind: "up-to-date" } },
        );
      } catch (error) {
        warn("check", error);
        set({ state: { kind: "error" } });
      }
      await version;
    },

    async checkOnLaunch() {
      if (get().hasChecked) return;
      await get().checkForUpdates();
    },

    async startDownload() {
      const current = get().state;
      if (current.kind !== "available" || handle === null) return;
      const { version } = current;
      const pending = handle;
      set({ state: { kind: "downloading", version, percent: null } });
      let total: number | null = null;
      let received = 0;
      try {
        await pending.download((event) => {
          if (event.event === "Started") {
            total = event.data.contentLength ?? null;
            received = 0;
          } else if (event.event === "Progress") {
            received += event.data.chunkLength;
          } else {
            return;
          }
          const percent = total === null || total <= 0 ? null : Math.min(100, Math.round((received / total) * 100));
          set({ state: { kind: "downloading", version, percent } });
        });
        set({ state: { kind: "ready-to-install", version } });
      } catch (error) {
        warn("download", error);
        set({ state: { kind: "error" } });
      }
    },

    async restart() {
      const pending = handle;
      const client = activeClient;
      if (restarting || !client || get().state.kind !== "ready-to-install" || pending === null) return;
      restarting = true;
      try {
        // Windows: hands off to the NSIS installer and exits; nothing below runs.
        await pending.install();
        await client.relaunch();
      } catch (error) {
        warn("restart", error);
        set({ state: { kind: "error" } });
      } finally {
        restarting = false;
      }
    },
  }));
}

export const updateStore = createUpdateStore();

export function useUpdateStore<T>(selector: (state: UpdateStore) => T): T {
  return useStore(updateStore, selector);
}

/** True while something actionable is pending — the only thing that lights the settings dot. */
export function hasPendingUpdate(state: UpdateState): boolean {
  return state.kind === "available" || state.kind === "downloading" || state.kind === "ready-to-install";
}
