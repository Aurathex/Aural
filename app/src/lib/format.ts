/** Download sizes as people read them: "82 MB", "661 MB", "2.4 GB" (decimal units). */
export function formatBytes(bytes: number): string {
  const mb = bytes / 1e6;
  if (mb < 1000) return `${Math.max(1, Math.round(mb))} MB`;
  return `${(bytes / 1e9).toFixed(1)} GB`;
}

export function percent(done: number, total: number): number {
  if (total <= 0) return 0;
  return Math.min(100, Math.max(0, Math.floor((done / total) * 100)));
}
