// Pomocnicze dla testów (node --test): teksty z i18n/ jak w build.mjs i małe „expect” w stylu Vitest.
import { readFileSync, readdirSync } from "node:fs";
import assert from "node:assert/strict";

const dir = new URL("../../i18n/", import.meta.url);
export const keysTsv = readFileSync(new URL("_keys.tsv", dir), "utf8");
export const files = Object.fromEntries(
  readdirSync(dir).filter((f) => f.endsWith(".txt")).sort().map((f) => [`i18n/${f}`, readFileSync(new URL(f, dir), "utf8")]),
);
globalThis.SHRIMP_I18N = { keys: keysTsv, files };

export const readSrc = (p) => readFileSync(new URL(p, import.meta.url), "utf8");

export function expect(v, msg) {
  return {
    toBe: (w) => assert.strictEqual(v, w, msg),
    toEqual: (w) => assert.deepStrictEqual(v, w, msg),
    toBeNull: () => assert.strictEqual(v, null, msg),
    toBeCloseTo: (w, digits = 2) => assert.ok(Math.abs(v - w) < 10 ** -digits / 2, msg ?? `${v} ≉ ${w}`),
    toBeGreaterThan: (w) => assert.ok(v > w, msg ?? `${v} ≤ ${w}`),
  };
}
