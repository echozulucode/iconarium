import type { BackendError } from "./types";

/** Error thrown by every Backend method. Satisfies the BackendError contract shape. */
export class AppError extends Error implements BackendError {
  readonly kind: string;
  constructor(kind: string, message: string) {
    super(message);
    this.name = "AppError";
    this.kind = kind;
  }
}

/** Normalize whatever a command rejected with (BackendError object, string, Error, unknown). */
export function normalizeError(e: unknown): AppError {
  if (e instanceof AppError) return e;
  if (typeof e === "string") {
    // Some serializers send JSON strings.
    const trimmed = e.trim();
    if (trimmed.startsWith("{")) {
      try {
        return normalizeError(JSON.parse(trimmed));
      } catch {
        /* fall through */
      }
    }
    return new AppError("error", e || "Unknown error");
  }
  if (e && typeof e === "object") {
    const o = e as Record<string, unknown>;
    const message =
      typeof o.message === "string"
        ? o.message
        : typeof o.error === "string"
          ? o.error
          : typeof o.msg === "string"
            ? o.msg
            : safeStringify(o);
    const kind = typeof o.kind === "string" ? o.kind : typeof o.code === "string" ? o.code : e instanceof Error ? e.name : "error";
    return new AppError(kind, message);
  }
  return new AppError("error", String(e ?? "Unknown error"));
}

function safeStringify(o: unknown): string {
  try {
    return JSON.stringify(o);
  } catch {
    return "Unknown error";
  }
}

/** User-facing message for an error. */
export function errorMessage(e: unknown): string {
  return normalizeError(e).message;
}
