import { readFileSync } from "node:fs";
import { defineConfig, type Plugin } from "vite";

const read = (f: string) => readFileSync(new URL(f, import.meta.url), "utf8");

/** Ikony i ekrany z makiety wstawiane do index.html. */
const markup = (): Plugin => ({
  name: "shrimp-markup",
  transformIndexHtml: (html) => html.replace("<!--icons-->", read("src/icons.svg")).replace("<!--screens-->", read("src/screens.html")),
});

/** Jeden plik HTML: program wkleja go do .exe jako zasób i ładuje przez NavigateToString. */
const singleFile = (): Plugin => ({
  name: "shrimp-single-file",
  apply: "build",
  enforce: "post",
  generateBundle(_, bundle) {
    const page = bundle["index.html"];
    if (!page || page.type !== "asset") return;
    let html = String(page.source);
    for (const [name, out] of Object.entries(bundle)) {
      if (out.type === "chunk" && out.isEntry) {
        html = html.replace(new RegExp(`<script type="module" crossorigin src="[^"]*${name}"></script>`),
          () => `<script type="module">${out.code.replace(/<\/script/g, "<\\/script")}</script>`);
        delete bundle[name];
      } else if (out.type === "asset" && name.endsWith(".css")) {
        html = html.replace(new RegExp(`<link rel="stylesheet" crossorigin href="[^"]*${name}">`), () => `<style>${out.source}</style>`);
        delete bundle[name];
      }
    }
    page.source = html;
  },
});

export default defineConfig({
  base: "./",
  plugins: [markup(), singleFile()],
  server: { fs: { allow: ["../.."] } },
  build: { outDir: "dist", assetsInlineLimit: () => true, cssCodeSplit: false, target: "es2022", modulePreload: false },
});
