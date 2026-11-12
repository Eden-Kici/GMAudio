/** 83000 -> "1:23", 3723000 -> "1:02:03". */
export function formatTime(ms: number | null | undefined): string {
  if (ms == null) return "–";
  const total = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const ss = String(s).padStart(2, "0");
  return h > 0 ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

/** The part of a file's folder below the imported root, e.g. "Taverns › Night". */
export function relativeFolder(assetPath: string, roots: string[]): string {
  const dir = assetPath.replace(/[\\/][^\\/]*$/, "");
  const root = roots.find((r) => dir.toLowerCase().startsWith(r.toLowerCase()));
  const rest = root ? dir.slice(root.length) : dir;
  return rest.split(/[\\/]/).filter(Boolean).join(" › ");
}

export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}

export function plural(n: number, one: string, many = `${one}s`): string {
  return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}
