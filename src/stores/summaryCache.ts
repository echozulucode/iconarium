// Per-asset summary cache with per-id subscriptions (cards subscribe only to their own id,
// so filling a page of summaries re-renders just the affected cards).
import { useSyncExternalStore } from "react";
import type { AssetId, AssetSummary } from "../api/types";
import { backend } from "../app/backendRef";

const PAGE = 200;

interface Entry {
  s: AssetSummary;
  gen: number;
}

function same(a: AssetSummary, b: AssetSummary): boolean {
  return (
    a.fingerprint === b.fingerprint &&
    a.state === b.state &&
    a.filename === b.filename &&
    a.relDir === b.relDir &&
    a.width === b.width &&
    a.height === b.height &&
    a.matchInfo?.field === b.matchInfo?.field &&
    a.matchInfo?.snippet === b.matchInfo?.snippet
  );
}

class SummaryCache {
  private map = new Map<AssetId, Entry>();
  private listeners = new Map<AssetId, Set<() => void>>();
  private staleListeners = new Set<() => void>();
  private pending = new Set<AssetId>();
  private gen = 0;
  private query = "";

  get(id: AssetId): AssetSummary | undefined {
    return this.map.get(id)?.s;
  }

  subscribe(id: AssetId, cb: () => void): () => void {
    let set = this.listeners.get(id);
    if (!set) this.listeners.set(id, (set = new Set()));
    set.add(cb);
    return () => {
      set!.delete(cb);
      if (set!.size === 0) this.listeners.delete(id);
    };
  }

  /** Notified when entries become stale and visible ranges should be re-ensured. */
  onStale(cb: () => void): () => void {
    this.staleListeners.add(cb);
    return () => void this.staleListeners.delete(cb);
  }

  /** Mark everything stale (new result set / catalog change). Existing data stays visible until refreshed. */
  invalidateAll(query: string): void {
    this.gen++;
    this.query = query;
    this.pending.clear();
    this.staleListeners.forEach((cb) => cb());
  }

  invalidate(ids: AssetId[]): void {
    let any = false;
    for (const id of ids) {
      const e = this.map.get(id);
      if (e) {
        e.gen = -1;
        any = true;
      }
    }
    if (any) this.staleListeners.forEach((cb) => cb());
  }

  private nonces = new Map<AssetId, number>();

  /** Thumbnail retry counter (bumped by thumb://ready so failed <img>s reload). */
  thumbNonce(id: AssetId): number {
    return this.nonces.get(id) ?? 0;
  }

  thumbsReady(ids: AssetId[]): void {
    for (const id of ids) {
      if (!this.listeners.has(id)) continue;
      this.nonces.set(id, (this.nonces.get(id) ?? 0) + 1);
      this.listeners.get(id)?.forEach((cb) => cb());
    }
    this.invalidate(ids);
  }

  clear(): void {
    this.nonces.clear();
    this.map.clear();
    this.pending.clear();
    this.gen++;
    this.listeners.forEach((set) => set.forEach((cb) => cb()));
  }

  /** Fetch missing/stale summaries for `ids` in pages. */
  ensure(ids: ArrayLike<AssetId>): void {
    const need: AssetId[] = [];
    for (let i = 0; i < ids.length; i++) {
      const id = ids[i];
      const e = this.map.get(id);
      if ((!e || e.gen !== this.gen) && !this.pending.has(id)) need.push(id);
    }
    if (need.length === 0) return;
    for (let i = 0; i < need.length; i += PAGE) {
      const page = need.slice(i, i + PAGE);
      void this.fetchPage(page);
    }
  }

  private async fetchPage(ids: AssetId[]): Promise<void> {
    const gen = this.gen;
    const query = this.query;
    ids.forEach((id) => this.pending.add(id));
    try {
      const rows = await backend().getAssets(ids, query);
      for (const s of rows) {
        const prev = this.map.get(s.id);
        // Store results even from an older generation (better than nothing) but keep them marked stale.
        const entryGen = gen === this.gen ? gen : -1;
        if (prev && same(prev.s, s)) {
          prev.gen = Math.max(prev.gen, entryGen);
          continue;
        }
        this.map.set(s.id, { s, gen: entryGen });
        this.listeners.get(s.id)?.forEach((cb) => cb());
      }
    } catch (e) {
      console.warn("getAssets failed", e);
    } finally {
      if (gen === this.gen) ids.forEach((id) => this.pending.delete(id));
    }
  }
}

export const summaryCache = new SummaryCache();

export function useSummary(id: AssetId): AssetSummary | undefined {
  return useSyncExternalStore(
    (cb) => summaryCache.subscribe(id, cb),
    () => summaryCache.get(id),
  );
}

export function useThumbNonce(id: AssetId): number {
  return useSyncExternalStore(
    (cb) => summaryCache.subscribe(id, cb),
    () => summaryCache.thumbNonce(id),
  );
}
