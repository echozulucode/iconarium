import type { Backend } from "../api/backend";

let current: Backend | null = null;

export function setBackend(b: Backend): void {
  current = b;
}

/** The resolved backend. Only valid after bootstrap (main.tsx awaits getBackend()). */
export function backend(): Backend {
  if (!current) throw new Error("Backend not initialized");
  return current;
}
