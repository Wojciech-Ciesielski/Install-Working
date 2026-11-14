// Buduje interfejs Shrimp Center do jednego pliku dist/index.html (program wkleja go do .exe i ładuje przez
// NavigateToString). Bez Vite i bez kompilatora TypeScript: moduły JS z src/ (i pliki .ts wspólnych boxów
// i18n-core, help-links, z których Node usuwa same typy) łączymy w jeden skrypt, CSS z czcionkami Inter
// wstawiamy jako <style>, teksty z i18n/ jako globalThis.SHRIMP_I18N.
//
//   node build.mjs            → dist/index.html
import { readFileSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import { dirname, join, relative, resolve, extname } from "node:path";
import { fileURLToPath } from "node:url";
import { stripTypeScriptTypes } from "node:module";

const here = dirname(fileURLToPath(import.meta.url));
const read = (p) => readFileSync(p, "utf8");

// ---------- skrypt: moduły ES jako funkcje w jednym pliku ----------
const IMPORT = /^import\s+(?:(.+?)\s+from\s+)?"([^"]+)"(?:\s+with\s*\{[^}]*\})?\s*;?[ \t]*$/gm;
const EXPORT = /^export\s+(?:async\s+)?(const|let|var|function\*?|class)\s+([A-Za-z_$][\w$]*)/gm;

const mods = new Map(); // ścieżka → kod funkcji modułu

function load(file) {
  if (mods.has(file)) return;
  mods.set(file, null);
  const id = JSON.stringify(relative(here, file));
  if (extname(file) === ".json") {
    mods.set(file, `${id}: () => ({ default: ${JSON.stringify(JSON.parse(read(file)))} })`);
    return;
  }
  let code = read(file);
  if (extname(file) === ".ts") code = stripTypeScriptTypes(code, { mode: "strip" });
  code = code.replace(IMPORT, (_, what, spec) => {
    const dep = resolve(dirname(file), spec);
    load(dep);
    const req = `__req(${JSON.stringify(relative(here, dep))})`;
    if (!what) return `${req};`;
    const out = [];
    const ns = /^\*\s+as\s+([\w$]+)$/.exec(what.trim());
    if (ns) return `const ${ns[1]} = ${req};`;
    const m = /^([\w$]+)?\s*,?\s*(?:\{([^}]*)\})?$/.exec(what.trim());
    if (!m) throw new Error(`${file}: nieobsługiwany import „${what}”`);
    if (m[1]) out.push(`const ${m[1]} = ${req}.default;`);
    const names = (m[2] || "").split(",").map((s) => s.trim()).filter((s) => s && !s.startsWith("type "));
    if (names.length) out.push(`const { ${names.map((n) => n.replace(/\s+as\s+/, ": ")).join(", ")} } = ${req};`);
    return out.join(" ");
  });
  const exported = [];
  code = code.replace(EXPORT, (_, kind, name) => { exported.push(name); return `${kind === "function*" ? "function*" : kind} ${name}`; });
  if (/^export\s/m.test(code)) throw new Error(`${file}: nieobsługiwany eksport (tylko export const/let/function/class)`);
  mods.set(file, `${id}: () => {\n${code}\nreturn { ${exported.join(", ")} };\n}`);
}

load(join(here, "src/main.js"));
const script = `(() => {
const __defs = {
${[...mods.values()].join(",\n")}
};
const __cache = {};
function __req(id) { if (!(id in __cache)) { __cache[id] = {}; __cache[id] = __defs[id](); } return __cache[id]; }
__req(${JSON.stringify("src/main.js")});
})();`;

// ---------- teksty ----------
const i18nDir = join(here, "../i18n");
const files = {};
for (const f of readdirSync(i18nDir).filter((f) => f.endsWith(".txt")).sort()) files[`i18n/${f}`] = read(join(i18nDir, f));
const texts = `globalThis.SHRIMP_I18N = ${JSON.stringify({ keys: read(join(i18nDir, "_keys.tsv")), files })};`;

// ---------- CSS: @import i czcionki jako data: ----------
function css(file) {
  return read(file)
    .replace(/@import\s+"([^"]+)"\s*;/g, (_, spec) => css(spec.startsWith(".") ? resolve(dirname(file), spec) : join(here, "node_modules", spec)))
    .replace(/url\((["']?)(\.[^)"']+)\1\)/g, (_, _q, p) => {
      const font = resolve(dirname(file), p);
      const type = { ".woff2": "font/woff2", ".woff": "font/woff", ".svg": "image/svg+xml", ".png": "image/png" }[extname(font)] || "application/octet-stream";
      return `url(data:${type};base64,${readFileSync(font).toString("base64")})`;
    });
}

// ---------- strona ----------
const noClose = (s) => s.replace(/<\/(script|style)/gi, "<\\/$1");
let html = read(join(here, "index.html"))
  .replace("<!--icons-->", () => read(join(here, "src/icons.svg")))
  .replace("<!--screens-->", () => read(join(here, "src/screens.html")))
  .replace(/<script type="module" src="[^"]*"><\/script>\n?/, "")
  .replace("</head>", () => `<style>${noClose(css(join(here, "src/style.css")))}</style>\n</head>`)
  .replace("</body>", () => `<script>${noClose(texts)}</script>\n<script>${noClose(script)}</script>\n</body>`);
mkdirSync(join(here, "dist"), { recursive: true });
writeFileSync(join(here, "dist/index.html"), html);
console.log(`dist/index.html: ${(html.length / 1024).toFixed(0)} KiB, ${mods.size} modułów`);
