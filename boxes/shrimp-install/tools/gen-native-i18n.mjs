#!/usr/bin/env node
// Komunikaty, które Shrimp Install pokazuje, zanim wystartuje interfejs (WebView2), z i18n/ boxa shrimp-install.
// Tworzy rufus/src/shrimp/native_i18n.h; `--check` tylko sprawdza, czy plik jest aktualny (CI).
// Znaki spoza ASCII jako \uXXXX, żeby kompilator nie zależał od kodowania źródeł.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const box = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const dir = path.join(box, "i18n");
const out = path.join(box, "rufus/src/shrimp/native_i18n.h");
const KEYS = ["native.wv2missing", "native.windowFail", "native.wv2fail", "native.noUi"];

const rows = (txt) => txt.split(/\r?\n/).filter((l) => l && !l.startsWith("#")).map((l) => l.split("\t"));
const keys = rows(fs.readFileSync(path.join(dir, "_keys.tsv"), "utf8"));
const ids = KEYS.map((k) => keys.find((r) => r[1] === k)?.[0] ?? (() => { throw new Error(`brak klucza ${k}`); })());
const en = ids.map((id) => keys.find((r) => r[0] === id)[2]);

const wlit = (s) => {
  let o = 'L"';
  for (const ch of s.replace(/\\n/g, "\n")) {
    const c = ch.codePointAt(0);
    if (ch === "\n") o += "\\n";
    else if (ch === '"' || ch === "\\") o += "\\" + ch;
    else if (c >= 0x20 && c < 0x7f) o += ch;
    else if (c > 0xffff) { const v = c - 0x10000; o += `\\u${(0xd800 + (v >> 10)).toString(16)}\\u${(0xdc00 + (v & 0x3ff)).toString(16)}`; }
    else o += "\\u" + c.toString(16).padStart(4, "0");
  }
  // \u przed cyfrą szesnastkową jest w porządku (zawsze 4 cyfry), ale znak ASCII nie może zaczynać się od \u
  return o + '"';
};

const langs = fs.readdirSync(dir).filter((f) => f.endsWith(".txt")).map((f) => f.slice(0, -4)).sort();
let h = "/* Wygenerowane przez boxes/shrimp-install/tools/gen-native-i18n.mjs z boxes/shrimp-install/i18n — nie edytuj ręcznie. */\n";
h += "#pragma once\n#include <wchar.h>\n\n";
h += `enum { ${KEYS.map((k) => "SN_" + k.slice(7).toUpperCase()).join(", ")}, SN_COUNT };\n\n`;
h += "typedef struct { const char* tag; const wchar_t* s[SN_COUNT]; } shrimp_native_tr;\n\n";
h += "static const shrimp_native_tr SHRIMP_NATIVE_TR[] = {\n";
h += `\t{ "en", { ${en.map(wlit).join(", ")} } },\n`;
for (const tag of langs) {
  const m = new Map(rows(fs.readFileSync(path.join(dir, tag + ".txt"), "utf8")).map((r) => [r[0], r[1]]));
  if (!ids.some((id) => m.has(id))) continue;
  h += `\t{ "${tag}", { ${ids.map((id, i) => (m.has(id) ? wlit(m.get(id)) : "NULL")).join(", ")} } },\n`;
}
h += "};\n";

if (process.argv.includes("--check")) {
  const cur = fs.existsSync(out) ? fs.readFileSync(out, "utf8") : "";
  if (cur !== h) { console.error("native_i18n.h jest nieaktualny: uruchom node boxes/shrimp-install/tools/gen-native-i18n.mjs"); process.exit(1); }
} else fs.writeFileSync(out, h);
