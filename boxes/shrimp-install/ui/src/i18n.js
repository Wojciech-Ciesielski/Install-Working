// Język interfejsu: wykryty w Windows (program przekazuje listę języków), z możliwością wymuszenia (?lang=, --lang).
// Teksty (i18n/_keys.tsv i <znacznik>.txt) wstawia build.mjs jako globalThis.SHRIMP_I18N; testy ładują je z dysku.
import { createI18n } from "../../../i18n-core/core.ts";

export const load = (requested) => {
  const d = globalThis.SHRIMP_I18N || { keys: "", files: {} };
  return createI18n(d.keys, d.files, requested);
};

/** Język z adresu (?lang=de), ustawień programu (window.SHRIMP_OPTS.langs) albo przeglądarki. */
export function requestedLanguages(q, opt) {
  const forced = q.get("lang") || opt.lang;
  return [...(forced ? [forced] : []), ...(opt.langs || []), ...(globalThis.navigator?.languages || [])];
}
