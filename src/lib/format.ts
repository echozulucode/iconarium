const nf = new Intl.NumberFormat("en-US");

export function formatCount(n: number): string {
  return nf.format(n);
}

export function plural(n: number, one: string, many = one + "s"): string {
  return `${formatCount(n)} ${n === 1 ? one : many}`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(n < 10 * 1024 ? 1 : 0)} KB`;
  if (n < 1024 * 1024 * 1024) return `${(n / (1024 * 1024)).toFixed(1)} MB`;
  return `${(n / (1024 * 1024 * 1024)).toFixed(2)} GB`;
}

export function formatDimension(n: number | null | undefined): string {
  if (n == null) return "–";
  return Number.isInteger(n) ? nf.format(n) : (Math.round(n * 100) / 100).toString();
}

export function formatDate(ms: number): string {
  return new Date(ms).toLocaleString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function formatRelativeTime(ms: number, now = Date.now()): string {
  const d = now - ms;
  const min = 60_000;
  if (d < min) return "just now";
  if (d < 60 * min) return `${Math.round(d / min)} min ago`;
  if (d < 24 * 60 * min) return `${Math.round(d / (60 * min))} h ago`;
  const days = Math.round(d / (24 * 60 * min));
  if (days < 30) return `${days} day${days === 1 ? "" : "s"} ago`;
  return new Date(ms).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });
}

export const isMac = typeof navigator !== "undefined" && /Mac/i.test(navigator.platform);
export const modKey = isMac ? "⌘" : "Ctrl";
