import { create } from "zustand";
import { errorMessage } from "../api/errors";
import type { AssetDetail, Region, ViewerBackground } from "../api/types";
import { backend } from "../app/backendRef";

export type ViewerMode = "pan" | "select";

interface ViewerState {
  openId: number | null;
  detail: AssetDetail | null;
  loading: boolean;
  error: string | null;
  mode: ViewerMode;
  region: Region | null;
  /** Session-only background override; null = use the settings default. */
  background: ViewerBackground | null;

  open(id: number): Promise<void>;
  close(): void;
  setMode(m: ViewerMode): void;
  setRegion(r: Region | null): void;
  setBackground(b: ViewerBackground): void;
}

let openSeq = 0;

export const useViewer = create<ViewerState>((set) => ({
  openId: null,
  detail: null,
  loading: false,
  error: null,
  mode: "pan",
  region: null,
  background: null,

  async open(id) {
    const my = ++openSeq;
    set({ openId: id, detail: null, loading: true, error: null, region: null });
    try {
      const d = await backend().getAssetDetail(id);
      if (my !== openSeq) return;
      set({ detail: d, loading: false });
    } catch (e) {
      if (my !== openSeq) return;
      set({ loading: false, error: errorMessage(e) });
    }
  },
  close() {
    openSeq++;
    set({ openId: null, detail: null, region: null, loading: false, error: null });
  },
  setMode(mode) {
    set({ mode });
  },
  setRegion(region) {
    set({ region });
  },
  setBackground(background) {
    set({ background });
  },
}));
