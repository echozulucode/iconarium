import { create } from "zustand";
import { errorMessage } from "../api/errors";
import type { LibraryInfo, ScanStatus, Settings } from "../api/types";
import { backend } from "../app/backendRef";
import { toast } from "./uiStore";

interface LibraryState {
  ready: boolean;
  library: LibraryInfo | null;
  recent: LibraryInfo[];
  scan: ScanStatus;
  settings: Settings | null;
  totalAssets: number;
  /** True while a library is being opened (picker shown / command in flight). */
  opening: boolean;

  init(): Promise<void>;
  refreshAppState(): Promise<void>;
  openLibrary(path: string): Promise<void>;
  pickLibrary(): Promise<void>;
  updateSettings(patch: Partial<Settings>): Promise<void>;
  setScan(s: ScanStatus): void;
  setTotal(n: number): void;
}

const IDLE: ScanStatus = { phase: "idle", discovered: 0, processed: 0, total: 0, message: "" };

export const useLibrary = create<LibraryState>((set, get) => ({
  ready: false,
  library: null,
  recent: [],
  scan: IDLE,
  settings: null,
  totalAssets: 0,
  opening: false,

  async init() {
    await get().refreshAppState();
    set({ ready: true });
  },

  async refreshAppState() {
    try {
      const s = await backend().getAppState();
      set({ library: s.library, recent: s.recent, scan: s.scan, settings: s.settings, totalAssets: s.totalAssets });
    } catch (e) {
      toast.error("Couldn't load application state", errorMessage(e));
    }
  },

  async openLibrary(path) {
    set({ opening: true });
    try {
      const lib = await backend().openLibrary(path);
      set({ library: lib, totalAssets: 0 });
      await get().refreshAppState();
    } catch (e) {
      toast.error("Couldn't open folder", errorMessage(e));
    } finally {
      set({ opening: false });
    }
  },

  async pickLibrary() {
    set({ opening: true });
    try {
      const lib = await backend().pickAndOpenLibrary();
      if (lib) {
        set({ library: lib, totalAssets: 0 });
        await get().refreshAppState();
      }
    } catch (e) {
      toast.error("Couldn't open folder", errorMessage(e));
    } finally {
      set({ opening: false });
    }
  },

  async updateSettings(patch) {
    const prev = get().settings;
    if (prev) set({ settings: { ...prev, ...patch } }); // optimistic
    try {
      const next = await backend().updateSettings(patch);
      set({ settings: next });
    } catch (e) {
      if (prev) set({ settings: prev });
      toast.error("Couldn't save settings", errorMessage(e));
    }
  },

  setScan(s) {
    set({ scan: s });
  },
  setTotal(n) {
    set({ totalAssets: n });
  },
}));
