// Czyste funkcje interfejsu (bez DOM), testowane w logic.test.ts. Teksty i formaty zależą od języka (i18n).
import type { I18n } from "../../../i18n-core/core";
import type { Device, Phase, Progress } from "./backend/types";

export const GiB = 1024 ** 3;
const MiB = 1024 ** 2;

/** Szacunkowe prędkości, zanim program zmierzy prawdziwe (B/s). */
export const EST = { download: 14 * MiB, write: 38 * MiB, copy: 52 * MiB, verify: 300 * MiB };

/** Rozmiar w GB z jedną cyfrą po przecinku. */
export const gb1 = (L: I18n, bytes: number) => L.unit(bytes / GiB, "gigabyte", { max: 1, min: 1 });

/** 29,8 GB · 465 GB · 1 TB (jednostki binarne, jak w Eksploratorze). */
export function fmtGB(L: I18n, bytes: number): string {
  const g = bytes / GiB;
  if (g >= 1000) {
    const t = g / 1024;
    return L.unit(t, "terabyte", { max: t < 10 ? 1 : 0, min: 0 });
  }
  return L.unit(g, "gigabyte", g >= 100 ? { max: 0 } : { max: 1, min: 1 });
}

/** Rozmiar pliku lub folderu: małe w MB. */
export function fmtSize(L: I18n, bytes: number): string {
  return bytes < 0.1 * GiB ? L.unit(Math.max(1, Math.round(bytes / MiB)), "megabyte") : gb1(L, bytes);
}

/** „około 12 minut”, „mniej niż minuta”. */
export function dur(L: I18n, sec: number): string {
  if (sec < 45) return L.t("dur.less");
  const m = Math.max(1, Math.round(sec / 60));
  return L.t("dur.about", { time: L.unit(m, "minute", { long: true }) });
}

export const mbps = (L: I18n, bps: number) => L.unit(Math.max(1, Math.round(bps / MiB)), "megabyte-per-second");

/** Wymaganie jak na opakowaniu: najmniejsza typowa pojemność (4, 8, 16, 32… GB), w którą wchodzi minSize.
 *  Pendrive „16 GB” ma naprawdę 14,5–16 mld bajtów, więc minSize jest niższy niż napis. */
export function minLabel(L: I18n, minSize: number): string {
  let g = 4;
  while (g * 0.9e9 < minSize) g *= 2;
  return L.unit(g, "gigabyte");
}

export const deviceLabel = (L: I18n, d: Device) => `${d.name} · ${fmtGB(L, d.size)}`;

export interface Space { shrPct: number; filesPct: number; text: string; over: boolean }

/** Pasek „Miejsce na nośniku”: Shrimp + kopia plików na nośniku o pojemności cap. */
export function space(L: I18n, cap: number, shrimp: number, files: number): Space {
  const free = cap - shrimp, left = free - files, over = left < 0;
  return {
    shrPct: Math.min(100, (shrimp / cap) * 100),
    filesPct: Math.max(0, (Math.min(files, Math.max(0, free)) / cap) * 100),
    text: over ? L.t("space.over", { size: gb1(L, -left) }) : L.t("space.free", { free: gb1(L, Math.max(0, left)), total: gb1(L, cap) }),
    over,
  };
}

/** Pozostały czas całości: bieżąca faza z pomiaru, kolejne z szacunku. */
export function remaining(p: Progress, sizes: { shrimp: number; files: number }): number {
  const left = Math.max(0, p.total - p.done);
  const est = [0, EST.download, EST.write, EST.copy];
  let s = left / (p.speed || (p.step === "verify" ? EST.verify : est[p.phase]));
  if (p.phase < 2 || p.step === "backup" || p.step === "prepare") s += sizes.shrimp / EST.write;
  if (p.phase < 3) s += sizes.files / EST.copy;
  return s;
}

/** Opis jednej fazy pod jej nazwą (jak w makiecie). */
export function phaseText(L: I18n, n: Phase, cur: Progress | null, sizes: { shrimp: number; files: number }, finished: boolean): string {
  const at = finished ? 4 : cur ? cur.phase : 1;
  const left = (p: Progress) => dur(L, Math.max(0, p.total - p.done) / (p.speed || [0, EST.download, EST.write, EST.copy][p.phase]));
  const progress = (p: Progress, speed: number) =>
    L.t("phase.progress", { done: L.num(p.done / GiB), total: gb1(L, p.total), speed: mbps(L, p.speed || speed), left: left(p) });
  if (n === 1) {
    if (at > 1) return L.t("phase.downloaded", { size: gb1(L, sizes.shrimp) });
    if (!cur || !cur.total) return L.t("phase.connecting");
    if (cur.step === "verify") return L.t("phase.verify", { pct: L.percent(cur.done / cur.total) });
    return progress(cur, EST.download);
  }
  if (n === 2) {
    if (at > 2) return L.t("phase.written");
    if (at < 2 || !cur) return L.t("phase.takes", { time: dur(L, sizes.shrimp / EST.write) });
    if (cur.step === "prepare") return L.t("phase.prepare");
    if (cur.step === "backup") return L.t("phase.backup", { pct: L.percent(cur.total ? cur.done / cur.total : 0), left: left(cur) });
    const pct = cur.total ? cur.done / cur.total : 0;
    return L.t("phase.percent", { pct: L.percent(pct), speed: mbps(L, cur.speed || EST.write), left: left(cur) });
  }
  if (!sizes.files) return at > 3 ? L.t("phase.nofiles") : L.t("phase.nothing");
  if (at > 3) return L.t("phase.copied", { size: fmtSize(L, sizes.files) });
  if (at < 3 || !cur) return L.t("phase.copyplan", { size: fmtSize(L, sizes.files), time: dur(L, sizes.files / EST.copy) });
  return progress(cur, EST.copy);
}

/** Ułamek ukończenia fazy n (0–1) dla paska. */
export function phaseFrac(n: Phase, cur: Progress | null, finished: boolean): number {
  if (finished) return 1;
  const at = cur ? cur.phase : 1;
  if (n < at) return 1;
  if (n > at || !cur || !cur.total) return 0;
  return Math.min(1, cur.done / cur.total);
}

/** Komunikat z programu: kod („connect”, „http:404”) → tłumaczenie; nieznany tekst zostaje bez zmian. */
export function hostMessage(L: I18n, m: string): string {
  const [code, arg] = m.split(":");
  return /^\w+$/.test(code) && L.has("host." + code) ? L.t("host." + code, { code: arg ?? "" }) : m;
}
