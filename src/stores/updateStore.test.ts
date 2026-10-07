import { afterEach, describe, expect, it, vi } from "vitest";
import {
  __setUpdaterClientForTests,
  createUpdateStore,
  hasPendingUpdate,
  type UpdateDownloadEvent,
  type UpdaterClient,
} from "./updateStore";

function fakeClient(over: Partial<UpdaterClient> = {}): UpdaterClient {
  return {
    getVersion: async () => "0.1.0",
    check: async () => null,
    relaunch: vi.fn(async () => {}),
    ...over,
  };
}

afterEach(() => __setUpdaterClientForTests(null));

describe("updateStore", () => {
  it("stays idle outside Tauri", async () => {
    __setUpdaterClientForTests(null);
    const s = createUpdateStore();
    await s.getState().checkOnLaunch();
    expect(s.getState().state.kind).toBe("idle");
  });

  it("reports up-to-date and the running version", async () => {
    __setUpdaterClientForTests(fakeClient());
    const s = createUpdateStore();
    await s.getState().checkOnLaunch();
    expect(s.getState().state.kind).toBe("up-to-date");
    expect(s.getState().currentVersion).toBe("0.1.0");
  });

  it("swallows check failures into the quiet error state", async () => {
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    __setUpdaterClientForTests(fakeClient({ check: async () => Promise.reject(new Error("offline")) }));
    const s = createUpdateStore();
    await s.getState().checkForUpdates();
    expect(s.getState().state.kind).toBe("error");
    expect(hasPendingUpdate(s.getState().state)).toBe(false);
    warn.mockRestore();
  });

  it("checks only once per session on launch", async () => {
    const check = vi.fn(async () => null);
    __setUpdaterClientForTests(fakeClient({ check }));
    const s = createUpdateStore();
    await s.getState().checkOnLaunch();
    await s.getState().checkOnLaunch();
    expect(check).toHaveBeenCalledTimes(1);
  });

  it("downloads with progress, then installs and relaunches", async () => {
    const install = vi.fn(async () => {});
    const percents: (number | null)[] = [];
    const client = fakeClient({
      check: async () => ({
        version: "0.2.0",
        notes: "notes",
        install,
        download: async (on: (e: UpdateDownloadEvent) => void) => {
          on({ event: "Started", data: { contentLength: 200 } });
          on({ event: "Progress", data: { chunkLength: 50 } });
          on({ event: "Progress", data: { chunkLength: 150 } });
          on({ event: "Finished" });
        },
      }),
    });
    __setUpdaterClientForTests(client);
    const s = createUpdateStore();
    s.subscribe((st) => {
      if (st.state.kind === "downloading") percents.push(st.state.percent);
    });
    await s.getState().checkForUpdates();
    expect(s.getState().state).toEqual({ kind: "available", version: "0.2.0", notes: "notes" });
    expect(hasPendingUpdate(s.getState().state)).toBe(true);
    await s.getState().startDownload();
    expect(percents).toEqual([null, 0, 25, 100]);
    expect(s.getState().state).toEqual({ kind: "ready-to-install", version: "0.2.0" });
    await s.getState().restart();
    expect(install).toHaveBeenCalledTimes(1);
    expect(client.relaunch).toHaveBeenCalledTimes(1);
  });
});
