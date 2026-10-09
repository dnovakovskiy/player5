// Output peak meter. The level is computed in the engine host from the
// rendered blocks (a decaying peak, ~20 updates per second); this only
// draws it, on a dBFS scale with a mark at the default −6 dBFS peak.

export const METER_FLOOR_DB = -48;

export function toDb(linear: number): number {
  return linear > 0 ? 20 * Math.log10(linear) : -Infinity;
}

export function formatDb(db: number, unit = "dBFS"): string {
  if (!Number.isFinite(db)) return `−∞ ${unit}`;
  const s = db.toFixed(1);
  return `${s.startsWith("-") ? "−" + s.slice(1) : s} ${unit}`;
}

export class Meter {
  constructor(
    private readonly el: HTMLElement,
    private readonly fill: HTMLElement,
    private readonly out: HTMLElement,
  ) {}

  set(peak: number): void {
    const db = toDb(peak);
    const shown = Number.isFinite(db) ? Math.max(METER_FLOOR_DB, Math.min(0, db)) : METER_FLOOR_DB;
    const frac = (shown - METER_FLOOR_DB) / -METER_FLOOR_DB;
    this.fill.style.clipPath = `inset(0 ${((1 - frac) * 100).toFixed(2)}% 0 0)`;
    this.el.dataset.peakDb = Number.isFinite(db) ? db.toFixed(1) : "-inf";
    this.el.dataset.zone = db >= -1 ? "hot" : db >= -6 ? "loud" : "ok";
    this.el.setAttribute("aria-valuenow", shown.toFixed(1));
    this.el.setAttribute("aria-valuetext", Number.isFinite(db) ? formatDb(db) : "silent");
    this.out.textContent = formatDb(db < METER_FLOOR_DB - 12 ? -Infinity : db);
  }
}
