// Start interfejsu. Parametry: ?screen=<ekran makiety>&theme=light|dark&shot (zrzuty: bez upływu czasu).
// Program może je też wstrzyknąć jako window.SHRIMP_OPTS (np. shrimp-center.exe --demo).
import { App } from "./app.js";
import { DemoBackend } from "./backend/demo.js";
import { HostBackend, hasHost } from "./backend/host.js";
import { load, requestedLanguages } from "./i18n.js";
const q = new URLSearchParams(location.search);
const opt = { ...window.SHRIMP_OPTS };
const screen = (q.get("screen") || opt.screen || "start");
const theme = q.get("theme") || opt.theme;
if (theme)
    document.documentElement.dataset.theme = theme;
const demo = {
    devices: screen === "brak-nosnika" ? "none" : screen === "za-maly" ? "small" : "ok",
    offline: screen === "brak-internetu",
    fail: screen === "nosnik-zajety" ? { phase: 2, kind: "busy" }
        : screen === "pytanie-rufusa" ? { phase: 2, kind: "question", message: "Revoked UEFI bootloader detected" } : undefined,
    at: screen === "postep" ? 24 : 0,
    frozen: q.has("shot"),
};
const real = hasHost() && !opt.demo && !q.has("demo");
// Język: z adresu/--lang, potem języki interfejsu Windows (program), potem przeglądarki. Brak dopasowania: angielski.
const L = load(requestedLanguages(q, opt));
const app = new App(real ? new HostBackend() : new DemoBackend(demo), L, !real);
if (!real && screen === "pliki-reczny")
    app.presetPicked(DemoBackend.PICKED);
app.init(real ? "start" : screen);
