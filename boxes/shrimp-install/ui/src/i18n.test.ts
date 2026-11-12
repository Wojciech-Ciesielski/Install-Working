import { describe, expect, it } from "vitest";
import { check, missingLanguages } from "../../../i18n-core/check";
import { negotiate, parseKeys } from "../../../i18n-core/core";
import { load } from "./i18n";
import keysTsv from "../../i18n/_keys.tsv?raw";
import screens from "./screens.html?raw";

const files = import.meta.glob("../../i18n/*.txt", { query: "?raw", import: "default", eager: true }) as Record<string, string>;
const sources = import.meta.glob(["./*.ts", "./backend/*.ts", "!./*.test.ts"], { query: "?raw", import: "default", eager: true }) as Record<string, string>;

describe("pliki tłumaczeń Shrimp Install", () => {
  const r = check(keysTsv, files, "win");
  it("są poprawne (id, parametry, puste teksty)", () => expect(r.errors).toEqual([]));
  it("polski jest kompletny", () => expect(r.coverage.pl).toBe(r.total));
  it("każdy użyty klucz istnieje", () => {
    const keys = new Set(parseKeys(keysTsv).map((k) => k.key));
    const used = new Set<string>();
    for (const src of Object.values(sources)) for (const m of src.matchAll(/\b(?:t|L\.t|this\.t)\(\s*["'`]([\w.]+)["'`]/g)) used.add(m[1]);
    for (const m of screens.matchAll(/data-i(?:-aria)?="([\w.]+)"/g)) used.add(m[1]);
    for (const f of ["documents", "pictures", "desktop", "music", "videos", "downloads"]) used.add("folder." + f);
    for (const f of ["offline", "verify", "space", "write", "device", "copy", "other"]) { used.add(`fail.${f}.t`); used.add(`fail.${f}.b`); }
    for (const p of [1, 2, 3]) used.add("phase." + p);
    for (const k of ["usb", "sd", "disk"]) used.add("kind." + k);
    expect([...used].filter((k) => !k.endsWith(".") && !keys.has(k))).toEqual([]);
  });
  it("brakujące języki (informacyjnie)", () => {
    console.log(`Shrimp Install: ${Object.keys(r.coverage).length + 1} języków z plikiem, brakuje ${missingLanguages(r, "win").length}`);
  });
});

describe("język zapasowy dla brakujących tekstów", () => {
  it("K’iche’ bez tłumaczenia sięga po hiszpański, a nie angielski", () => {
    const es = load(["es"]);
    expect(load(["quc"]).t("start.go")).toBe(es.t("start.go"));
    expect(load(["quc"]).lang).toBe("quc");
  });
  it("język bez zapasowego zostaje przy angielskim", () => {
    expect(load(["chr"]).t("start.go")).toBe(load(["en"]).t("start.go"));
  });
});

describe("wykrywanie języka", () => {
  it("dopasowuje znaczniki Windows", () => {
    expect(negotiate(["pl-PL"])).toBe("pl");
    expect(negotiate(["en-GB", "pl"])).toBe("en");
    expect(negotiate(["zh-CN"])).toBe("zh-hans");
    expect(negotiate(["zh-Hans-CN"])).toBe("zh-hans");
    expect(negotiate(["zh-TW"])).toBe("zh-hant");
    expect(negotiate(["zh-HK"])).toBe("zh-hant");
    expect(negotiate(["pt-BR"])).toBe("pt-br");
    expect(negotiate(["pt-PT"])).toBe("pt-pt");
    expect(negotiate(["pt"])).toBe("pt-br");
    expect(negotiate(["sr-Latn-RS"])).toBe("sr-latn");
    expect(negotiate(["sr-Cyrl-RS"])).toBe("sr-cyrl");
    expect(negotiate(["ca-ES-valencia"])).toBe("ca-valencia");
    expect(negotiate(["ca-ES"])).toBe("ca");
    expect(negotiate(["nb-NO"])).toBe("nb");
    expect(negotiate(["no"])).toBe("nb");
    expect(negotiate(["nn-NO"])).toBe("nn");
    expect(negotiate(["ku-Arab-IQ"])).toBe("ckb");
    expect(negotiate(["pa-Arab-PK"])).toBe("pa-arab");
    expect(negotiate(["pa-IN"])).toBe("pa");
    expect(negotiate(["uz-Latn-UZ"])).toBe("uz");
    expect(negotiate(["quc-Latn-GT"])).toBe("quc");
    expect(negotiate(["chr-Cher-US"])).toBe("chr");
    expect(negotiate(["fil-PH"])).toBe("fil");
    expect(negotiate(["es-MX"])).toBe("es");
    expect(negotiate(["fr-CA"])).toBe("fr");
  });
  it("przyjmuje też identyfikatory GNOME", () => {
    expect(negotiate(["pl_PL.UTF-8"])).toBe("pl");
    expect(negotiate(["sr_RS@latin"])).toBe("sr-latn");
    expect(negotiate(["ca_ES@valencia"])).toBe("ca-valencia");
    expect(negotiate(["uz_UZ@cyrillic"])).toBe("uz-cyrl");
  });
  it("nieznany język: angielski", () => {
    expect(negotiate(["xx-YY"])).toBe("en");
    expect(negotiate([])).toBe("en");
  });
  it("kierunek pisma i atrybut lang", () => {
    expect(load(["ar-SA"]).rtl).toBe(true);
    expect(load(["he-IL"]).rtl).toBe(true);
    expect(load(["pl"]).rtl).toBe(false);
    expect(load(["zh-TW"]).lang).toBe("zh-Hant");
    expect(load(["ab"]).locale).toBe("ru");
    expect(load(["quc-Latn-GT"]).locale).toBe("es-GT");
  });
});
