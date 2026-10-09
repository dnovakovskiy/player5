// localStorage that never throws: sandboxed frames, opaque origins and
// privacy modes make the accessor itself or any call fail. Only per-viewer
// conveniences live here; the pattern itself lives in the URL.

const PREFIX = "player5:";

function store(): Storage | null {
  try {
    return window.localStorage ?? null;
  } catch {
    return null;
  }
}

export function loadString(key: string): string | null {
  try {
    return store()?.getItem(PREFIX + key) ?? null;
  } catch {
    return null;
  }
}

export function saveString(key: string, value: string | null): void {
  try {
    const s = store();
    if (!s) return;
    if (value === null) s.removeItem(PREFIX + key);
    else s.setItem(PREFIX + key, value);
  } catch {
    /* storage full or blocked: preferences are optional */
  }
}

export function loadNumber(key: string, fallback: number): number {
  const v = loadString(key);
  const n = v === null ? NaN : Number(v);
  return Number.isFinite(n) ? n : fallback;
}
