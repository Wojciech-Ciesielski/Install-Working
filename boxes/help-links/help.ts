// Adresy stron pomocy (boxes/help-links/help.json), wspólne dla Shrimp Install i Setup.
import cfg from "./help.json";

/**
 * Adres strony pomocy dla tematu albo null, gdy strony jeszcze nie ma (puste `base`) lub adres jest niebezpieczny.
 * `base` z parametru (np. z manifestu wydania) ma pierwszeństwo przed wpisem w help.json.
 */
export function helpUrl(topic: string, lang: string, from: "install" | "setup", base: string = cfg.base): string | null {
  const path = (cfg.topics as Record<string, string>)[topic];
  const b = (base || cfg.base || "").trim().replace(/\/+$/, "");
  if (!path || !/^https:\/\/[a-z0-9.-]+(:\d+)?(\/[A-Za-z0-9._~\/-]*)?$/i.test(b)) return null;
  return `${b}/${path}?lang=${encodeURIComponent(lang || "en")}&from=${from}`;
}
