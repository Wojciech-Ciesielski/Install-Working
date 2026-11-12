// Sprawdzanie plików tłumaczeń (używają go testy obu interfejsów): błędy budowy i pokrycie.
import { parseKeys, placeholders } from "./core";
import { LANGS } from "./langs";

export interface Report {
  errors: string[];
  /** tag → liczba przetłumaczonych kluczy. */
  coverage: Record<string, number>;
  total: number;
}

/** `which`: „win” (Shrimp Install) albo „gnome” (Setup): języki, które ta przestrzeń ma obsługiwać. */
export function check(keysTsv: string, files: Record<string, string>, which: "win" | "gnome"): Report {
  const keys = parseKeys(keysTsv);
  const byId = new Map(keys.map((k) => [k.id, k]));
  const errors: string[] = [];
  const coverage: Record<string, number> = {};
  const ids = new Set<number>();
  for (const k of keys) {
    if (ids.has(k.id)) errors.push(`_keys.tsv: powtórzone id ${k.id}`);
    ids.add(k.id);
  }
  for (const [path, txt] of Object.entries(files)) {
    const m = /([^/\\]+)\.txt$/.exec(path);
    if (!m) continue;
    const tag = m[1];
    if (!LANGS.some((l) => l.tag === tag)) errors.push(`${tag}.txt: nieznany język (dopisz go w boxes/i18n-core/langs.ts)`);
    else if (!LANGS.find((l) => l.tag === tag)![which]) errors.push(`${tag}.txt: ten język nie należy do listy „${which}”`);
    const seen = new Set<number>();
    let n = 0;
    txt.split(/\r?\n/).forEach((line, i) => {
      if (!line || line.startsWith("#")) return;
      const t = line.indexOf("\t");
      const where = `${tag}.txt:${i + 1}`;
      if (t < 1) return void errors.push(`${where}: brak „id⇥tekst”`);
      const id = +line.slice(0, t), val = line.slice(t + 1);
      const k = byId.get(id);
      if (!k) return void errors.push(`${where}: nieznane id ${id}`);
      if (seen.has(id)) errors.push(`${where}: powtórzone id ${id}`);
      seen.add(id);
      if (!val.trim()) return void errors.push(`${where}: pusty tekst (id ${id}, ${k.key})`);
      if (/\t/.test(val)) errors.push(`${where}: tabulator w tekście (id ${id})`);
      const a = placeholders(k.en).join(), b = placeholders(val).join();
      if (a !== b) errors.push(`${where}: parametry {…} inne niż w wzorcu (id ${id}, ${k.key}): „${b}” zamiast „${a}”`);
      if (/<[a-z/][^>]*>/i.test(val) && !/<[a-z/][^>]*>/i.test(k.en)) errors.push(`${where}: znaczniki HTML w tekście (id ${id})`);
      n++;
    });
    coverage[tag] = n;
  }
  return { errors, coverage, total: keys.length };
}

/** Języki, które ta przestrzeń ma obsługiwać, a nie mają jeszcze pliku (angielski jest wzorcem, polski mamy od początku). */
export function missingLanguages(report: Report, which: "win" | "gnome"): string[] {
  return LANGS.filter((l) => l[which] && l.tag !== "en" && !(l.tag in report.coverage)).map((l) => l.tag);
}
