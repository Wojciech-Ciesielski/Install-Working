// Wspólna warstwa tłumaczeń Shrimp Install i Setup (opis formatu: boxes/i18n-core/README.md).
//
// Pliki:
//   <przestrzeń>/_keys.tsv   „id⇥klucz⇥angielski” (id są stałe, nowe dopisujemy na końcu)
//   <przestrzeń>/<tag>.txt   „id⇥tłumaczenie” (brakujące wiersze = angielski; „\n” to nowy wiersz)
import { LANGS, type LangInfo } from "./langs";

export type Args = Record<string, string | number>;

export interface Entry { id: number; key: string; en: string }

/** `_keys.tsv` → wpisy. Puste wiersze i komentarze (#) są pomijane. */
export function parseKeys(tsv: string): Entry[] {
  const out: Entry[] = [];
  for (const line of tsv.split(/\r?\n/)) {
    if (!line || line.startsWith("#")) continue;
    const [id, key, ...en] = line.split("\t");
    out.push({ id: +id, key, en: unescape(en.join("\t")) });
  }
  return out;
}

/** `<tag>.txt` → id → tekst. */
export function parseValues(txt: string): Map<number, string> {
  const m = new Map<number, string>();
  for (const line of txt.split(/\r?\n/)) {
    if (!line || line.startsWith("#")) continue;
    const i = line.indexOf("\t");
    if (i < 1) continue;
    m.set(+line.slice(0, i), unescape(line.slice(i + 1)));
  }
  return m;
}

const unescape = (s: string) => s.replace(/\\n/g, "\n");

/** Nazwy parametrów {x} użyte w tekście. */
export const placeholders = (s: string): string[] => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

/** Najlepsza zgodność znacznika BCP-47 (albo identyfikatora locale GNOME: pl_PL.UTF-8, sr@latin) z naszym językiem. */
export function resolveTag(req: string, langs: LangInfo[] = LANGS): string | null {
  const MOD: Record<string, string> = { latin: "latn", cyrillic: "cyrl", arabic: "arab" };
  const parts = req.trim().toLowerCase().replace(/\..*?(?=@|$)/, "").replace(/@(\w+)/, (_, m) => "-" + (MOD[m] || m)).replace(/_/g, "-").split("-");
  const script = parts.find((p, i) => i > 0 && /^[a-z]{4}$/.test(p));
  const cands: string[] = [];
  for (let n = parts.length; n > 0; n--) cands.push(parts.slice(0, n).join("-"));
  if (script) cands.splice(1, 0, `${parts[0]}-${script}`);
  for (const c of cands) {
    const hit = langs.find((l) => l.tag === c || l.match?.includes(c));
    if (hit) return hit.tag;
  }
  return null;
}

/** Pierwszy z żądanych języków, który mamy. Bez dopasowania: angielski. */
export function negotiate(requested: string[], langs: LangInfo[] = LANGS): string {
  for (const r of requested) {
    const t = resolveTag(r, langs);
    if (t) return t;
  }
  return "en";
}

export class I18n {
  /** Nasz identyfikator języka (np. „pt-br”). */
  readonly tag: string;
  /** Znacznik dla atrybutu lang (np. „pt-BR”). */
  readonly lang: string;
  /** Znacznik dla Intl (liczby, jednostki): jak lang, a gdy ICU nie zna języka, jego zapasowy odpowiednik. */
  readonly locale: string;
  readonly rtl: boolean;
  private dict = new Map<string, string>();

  constructor(tag: string, keys: Entry[], values: Map<number, string> | undefined, private fallback?: Map<string, string>) {
    this.tag = tag;
    const info = LANGS.find((l) => l.tag === tag);
    this.rtl = !!info?.rtl;
    this.lang = info?.bcp47 || tag;
    const known = (l: string) => { try { return Intl.NumberFormat.supportedLocalesOf([l]).length > 0; } catch { return false; } };
    this.locale = known(this.lang) ? this.lang : info?.intl && known(info.intl) ? info.intl : "en";
    for (const k of keys) this.dict.set(k.key, values?.get(k.id) ?? fallback?.get(k.key) ?? k.en);
  }

  /** Tekst o danym kluczu z podstawionymi {parametrami}. Nieznany klucz zwraca sam klucz. */
  t = (key: string, args?: Args): string => {
    const s = this.dict.get(key) ?? this.fallback?.get(key) ?? key;
    return args ? s.replace(/\{(\w+)\}/g, (m, n) => (n in args ? String(args[n]) : m)) : s;
  };
  has = (key: string) => this.dict.has(key);
  keys = () => [...this.dict.keys()];

  /** Liczba z jednostką w języku interfejsu: „29,8 GB”, „12 minut”, „14 MB/s”. max/min to liczba miejsc po przecinku. */
  unit(n: number, unit: string, o: { max?: number; min?: number; long?: boolean } = {}): string {
    const max = o.max ?? 0;
    try {
      return new Intl.NumberFormat(this.locale, {
        style: "unit", unit, unitDisplay: o.long ? "long" : "short",
        maximumFractionDigits: max, minimumFractionDigits: o.min ?? max,
      }).format(n);
    } catch {
      return n.toFixed(max) + " " + unit;
    }
  }
  /** Liczba z częścią dziesiętną zgodną z językiem. */
  num(n: number, digits = 1): string {
    try { return new Intl.NumberFormat(this.locale, { minimumFractionDigits: digits, maximumFractionDigits: digits }).format(n); }
    catch { return n.toFixed(digits); }
  }
  /** Ułamek jako procent („38%”, „38 %”, „٣٨٪؜”). */
  percent(frac: number): string {
    try { return new Intl.NumberFormat(this.locale, { style: "percent", maximumFractionDigits: 0 }).format(frac); }
    catch { return Math.round(frac * 100) + "%"; }
  }
}

/** Język zapasowy dla brakujących tekstów (pole `text` w langs.ts), np. „quz” → „es”; brak: angielski. */
export const textFallback = (tag: string): string | undefined => {
  const t = LANGS.find((l) => l.tag === tag)?.text;
  return t && t !== "en" && t !== tag ? t : undefined;
};

/** Buduje tłumacza dla wybranego języka z już wczytanego tekstu pliku (undefined: angielski).
 *  `fbTxt`: plik języka zapasowego (textFallback), którego teksty zastępują brakujące przed angielskim. */
export function build(keysTsv: string, tag: string, txt?: string, fbTxt?: string): I18n {
  const keys = parseKeys(keysTsv);
  const english = new I18n("en", keys, undefined);
  const fb = new Map(english.keys().map((k) => [k, english.t(k)]));
  if (fbTxt) {
    const v = parseValues(fbTxt);
    for (const k of keys) if (v.has(k.id)) fb.set(k.key, v.get(k.id)!);
  }
  return new I18n(tag, keys, txt ? parseValues(txt) : undefined, fb);
}

/** Wczytuje przestrzeń nazw (install/setup) dla żądanych języków. */
export function createI18n(keysTsv: string, files: Record<string, string>, requested: string[]): I18n {
  const byTag = new Map<string, string>();
  for (const [path, txt] of Object.entries(files)) {
    const m = /([^/\\]+)\.txt$/.exec(path);
    if (m && !m[1].startsWith("_")) byTag.set(m[1], txt);
  }
  const tag = negotiate(requested);
  const fb = textFallback(tag);
  return build(keysTsv, tag, byTag.get(tag), fb ? byTag.get(fb) : undefined);
}
