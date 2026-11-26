export function bytes(n: number): string {
  const units = ["B", "KB", "MB", "GB", "TB"];
  let v = n;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return i === 0 ? `${n} B` : `${v.toFixed(v >= 100 ? 0 : 1)} ${units[i]}`;
}

export function count(n: number): string {
  return n.toLocaleString();
}

export function ago(unixSecs: number): string {
  if (!unixSecs) return "—";
  const days = (Date.now() / 1000 - unixSecs) / 86400;
  if (days < 1) return "today";
  if (days < 2) return "yesterday";
  if (days < 30) return `${Math.floor(days)} days ago`;
  if (days < 365) return `${Math.floor(days / 30)} mo ago`;
  const y = days / 365;
  return `${y.toFixed(y < 10 ? 1 : 0)} yr ago`;
}

export function duration(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  const s = ms / 1000;
  if (s < 60) return `${s.toFixed(1)} s`;
  return `${Math.floor(s / 60)} min ${Math.round(s % 60)} s`;
}

export function baseName(p: string): string {
  const parts = p.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? p;
}

export function dirName(p: string): string {
  const i = Math.max(p.lastIndexOf("\\"), p.lastIndexOf("/"));
  return i > 0 ? p.slice(0, i) : p;
}
