// Shrimp Center: ekrany z makiety (mockups/shrimp-install.html) sterowane przez backend (umowa: backend/types.js).
import { helpUrl } from "../../../help-links/help.ts";
import { deviceLabel, dur, fmtGB, fmtSize, hostMessage, minLabel, phaseFrac, phaseText, remaining, space } from "./logic.js";
const $ = (s, r = document) => r.querySelector(s);
const $$ = (s, r = document) => Array.from(r.querySelectorAll(s));
const esc = (s) => s.replace(/[&<>"]/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c]);
const FOLDER_ICON = {
    documents: "#f-document", pictures: "#f-image", desktop: "#f-desktop", music: "#f-music_note_1", videos: "#f-video", downloads: "#f-arrow_download",
};
export class App {
    be;
    L;
    preview;
    info;
    devices = [];
    sel = null;
    folders = [];
    touched = false;
    mode = "auto";
    picked = [];
    cur = null;
    finished = false;
    failed = null;
    running = false;
    closeAfterCancel = false;
    screen = "start";
    installScreen = "start";
    sys = null;
    onConfirm = null;
    constructor(be, L, preview = false) {
        this.be = be;
        this.L = L;
        this.preview = preview;
    }
    t = (k, a) => this.L.t(k, a);
    /** Język interfejsu: atrybuty dokumentu i wszystkie stałe teksty z markup (data-i, data-i-aria, data-step). */
    applyStatic() {
        const L = this.L;
        document.documentElement.lang = L.lang;
        document.documentElement.dir = L.rtl ? "rtl" : "ltr";
        $$("[data-i]").forEach((el) => { el.textContent = L.t(el.dataset.i); });
        $$("[data-i-aria]").forEach((el) => el.setAttribute("aria-label", L.t(el.dataset.iAria)));
        $$("[data-step]").forEach((el) => { el.textContent = L.t("step", { n: L.num(+el.dataset.step, 0), total: L.num(4, 0) }); });
    }
    async init(first = "start") {
        this.info = await this.be.info();
        this.applyStatic();
        this.folders = this.info.folders.map((f) => ({ ...f, on: false }));
        this.defaults();
        $("#bitlocker").hidden = !this.info.bitlocker;
        this.initHelp();
        this.be.onDevices((d) => { this.devices = d; this.pickDevice(); this.drawCb(); });
        this.be.onFolders((f) => {
            this.folders = f.map((x) => ({ ...x, on: this.folders.find((o) => o.key === x.key)?.on ?? false }));
            if (!this.touched)
                this.defaults();
            if (this.screen === "pliki")
                this.drawFolders();
        });
        this.be.onBitLocker((on) => { $("#bitlocker").hidden = !on; });
        this.be.onCloseRequest(() => { if (this.running) {
            this.closeAfterCancel = true;
            this.openDlg();
        } });
        this.wire();
        this.go(first);
        if (first === "systemy" || first === "odinstaluj")
            this.loadSystems();
    }
    // ---------- nośnik ----------
    fits = (d) => d.size >= this.info.minSize;
    pickDevice() {
        if (this.sel !== null && this.devices.some((d) => d.id === this.sel && this.fits(d)))
            return;
        this.sel = this.devices.find(this.fits)?.id ?? null;
    }
    get device() { return this.devices.find((d) => d.id === this.sel) || null; }
    drawCb() {
        const cb = $("#cb"), val = $("#cbVal"), note = $("#cbNote"), pop = $("#cbPop");
        const d = this.device, any = this.devices.length > 0;
        cb.disabled = !any;
        val.className = "cbv";
        note.textContent = "";
        const min = minLabel(this.L, this.info.minSize);
        $("#mediaSub").textContent = this.t("media.sub", { size: min });
        if (d)
            val.textContent = deviceLabel(this.L, d);
        else if (any) {
            val.textContent = this.t("media.pick");
            val.className = "cbv ph";
            note.textContent = this.t("media.small", { size: min });
        }
        else {
            val.textContent = this.t("media.none");
            val.className = "cbv ph";
            note.textContent = this.t("media.plug");
        }
        $("#cbScan").hidden = any;
        $("#toFiles").disabled = !d;
        pop.innerHTML = this.devices.map((x) => {
            const dis = !this.fits(x);
            return `<button class="opt" type="button" role="option" data-id="${x.id}" aria-selected="${x.id === this.sel}"${dis ? ' aria-disabled="true"' : ""}>` +
                `<span class="n">${esc(x.name)} <span class="sz">· ${esc(fmtGB(this.L, x.size))}</span></span>` +
                (dis ? `<span class="d">${esc(this.t("media.toosmall"))}</span>` : "") + "</button>";
        }).join("");
        if (!pop.hidden && !any)
            this.closePop(false);
        if (this.screen === "pliki")
            this.drawSpace();
    }
    placePop() {
        const pop = $("#cbPop"), host = $(".win").getBoundingClientRect(), r = $("#cb").getBoundingClientRect();
        pop.style.width = r.width + "px";
        pop.style.left = r.left - host.left + "px";
        const h = pop.offsetHeight, below = host.bottom - r.bottom - 8;
        pop.style.top = (below >= h || r.top - host.top < h ? r.bottom - host.top + 4 : r.top - host.top - h - 4) + "px";
    }
    openPop() {
        const cb = $("#cb"), pop = $("#cbPop");
        if (cb.disabled || !this.devices.length)
            return;
        pop.hidden = false;
        cb.setAttribute("aria-expanded", "true");
        this.placePop();
        ($('.opt[aria-selected="true"]', pop) || $(".opt:not([aria-disabled])", pop))?.focus();
    }
    closePop(back) {
        $("#cbPop").hidden = true;
        $("#cb").setAttribute("aria-expanded", "false");
        if (back)
            $("#cb").focus();
    }
    // ---------- pliki ----------
    freeForFiles() { return (this.device?.size ?? 0) - this.info.shrimpSize; }
    /** Domyślnie zaznaczone foldery, które się mieszczą (w kolejności). */
    defaults() {
        let left = this.freeForFiles();
        if (!this.device)
            left = 28.7 * 1024 ** 3 - this.info.shrimpSize;
        for (const f of this.folders) {
            f.on = f.size !== null && f.size <= left;
            if (f.on)
                left -= f.size;
        }
    }
    used() {
        return this.mode === "auto"
            ? this.folders.reduce((a, f) => a + (f.on ? f.size || 0 : 0), 0)
            : this.picked.reduce((a, p) => a + p.size, 0);
    }
    drawSpace() {
        const cap = this.device?.size ?? 0;
        const s = space(this.L, cap || 1, this.info.shrimpSize, this.used());
        $("#spShr").style.width = s.shrPct + "%";
        $("#spFiles").style.width = s.filesPct + "%";
        $("#spTxt").textContent = s.text;
        const counting = this.mode === "auto" && this.folders.some((f) => f.on && f.size === null);
        $("#goBtn").disabled = !this.device || s.over || counting || (this.mode === "manual" && !this.picked.length);
    }
    drawFolders() {
        const u = this.used(), free = this.freeForFiles();
        $("#folders").innerHTML = this.folders.map((f, i) => {
            const name = esc(this.t("folder." + f.key)), ic = FOLDER_ICON[f.key];
            const blocked = !f.on && f.size !== null && u + f.size > free;
            const sz = f.size === null ? this.t("folder.counting") : blocked ? this.t("folder.nospace") : fmtSize(this.L, f.size);
            return `<button class="fol" type="button" role="switch" aria-checked="${f.on}" aria-label="${esc(this.t("folder.keep.aria", { name }))}" data-i="${i}"${blocked || f.size === null ? " disabled" : ""}>` +
                `<svg class="i ck"><use href="${f.on ? "#f-checkmark_circle-filled" : "#f-circle"}"/></svg><span class="fi"><svg class="i"><use href="${ic}"/></svg></span>` +
                `<span class="fn">${name}</span><span class="fs">${esc(sz)}</span></button>`;
        }).join("");
        this.drawSpace();
    }
    drawPicked() {
        $("#picked").innerHTML = this.picked.length
            ? this.picked.map((x, i) => `<div class="row"><svg class="i lg"><use href="${x.kind === "folder" ? "#f-folder" : "#f-document"}"/></svg><div class="tx"><div class="tt">${esc(x.name)}</div></div>` +
                `<span class="sz2">${esc(fmtSize(this.L, x.size))}</span><button class="link" type="button" data-rm="${i}" aria-label="${esc(this.t("files.remove.aria", { name: x.name }))}">${esc(this.t("files.remove"))}</button></div>`).join("")
            : `<p class="empty">${esc(this.t("files.empty"))}</p>`;
        this.drawSpace();
    }
    setMode(m, animate) {
        this.mode = m;
        $$(".mode").forEach((b) => b.setAttribute("aria-checked", String(b.dataset.mode === m)));
        $("#paneAuto").hidden = m !== "auto";
        $("#paneManual").hidden = m !== "manual";
        this.drawFolders();
        this.drawPicked();
        if (animate)
            rise(m === "auto" ? $$(".fol") : $$("#paneManual .adds .btn, #picked .row, #picked .empty"));
    }
    async add(kind) {
        const got = await this.be.pick(kind);
        const fresh = got.filter((g) => !this.picked.some((p) => p.path.toLowerCase() === g.path.toLowerCase()));
        if (!fresh.length)
            return;
        this.picked.push(...fresh);
        this.drawPicked();
        rise($$("#picked .row").slice(-fresh.length));
    }
    // ---------- postęp ----------
    sizes() { return { shrimp: this.info.shrimpSize, files: this.used() }; }
    drawRun() {
        const STI = { wait: "#f-circle", active: "#f-circle", done: "#f-checkmark_circle-filled", fail: "#f-error_circle" };
        const at = this.finished ? 4 : this.cur?.phase ?? 1;
        for (const n of [1, 2, 3]) {
            const el = $("#ph" + n);
            const st = this.failed?.phase === n ? "fail" : n < at ? "done" : n === at ? "active" : "wait";
            el.className = "row phase " + st;
            // Ikona tylko przy zmianie stanu (postęp przychodzi kilka razy na sekundę, a kółko w trakcie się kręci).
            const ic = $(".st", el);
            if (ic.dataset.st !== st) {
                ic.dataset.st = st;
                ic.innerHTML = st === "active" ? `<span class="spin" aria-hidden="true"></span>` : `<svg class="i lg"><use href="${STI[st]}"/></svg>`;
            }
            // Bez znanego postępu (łączenie, analiza obrazu, formatowanie) pasek przesuwa się w miejscu zamiast stać na 0%.
            const c = this.cur;
            const busy = st === "active" && (c === null ? n === 1 : c.phase === n && (c.step === "prepare" || !c.total));
            $("#b" + n).parentElement.classList.toggle("ind", busy);
            $("#b" + n).style.width = busy ? "" : phaseFrac(n, this.cur, this.finished) * 100 + "%";
            $(".pb-wrap", el).style.display = st === "fail" ? "none" : "";
            $("#n" + n).textContent = st === "fail" ? this.t(`fail.${this.failed.kind}.t`) : phaseText(this.L, n, this.cur, this.sizes(), this.finished);
        }
        const err = !!this.failed;
        $("#pgErr").hidden = !err;
        $("#pgInfo").hidden = err || this.finished;
        $("#retryBtn").hidden = !err;
        $("#nextBtn").hidden = err;
        $("#nextBtn").disabled = !this.finished;
        $("#cancelBtn").hidden = this.finished;
        if (err) {
            const f = this.failed, k = `fail.${f.kind}.`;
            $("#pgErrTxt").innerHTML = `<b>${esc(this.t(k + "b"))}</b>` + (this.L.has(k + "r") ? " " + esc(this.t(k + "r")) : "") + (f.message ? " " + esc(hostMessage(this.L, f.message)) : "");
            $("#pgTitle").textContent = this.t("run.title");
            $("#pgSub").textContent = this.t("run.sub.paused");
        }
        else if (this.finished) {
            $("#pgTitle").textContent = this.t("run.done.title");
            $("#pgSub").textContent = this.t("run.done.sub");
        }
        else {
            $("#pgTitle").textContent = this.t("run.title");
            const left = this.cur ? remaining(this.cur, this.sizes()) : remaining({ phase: 1, done: 0, total: this.info.shrimpSize, speed: 0 }, this.sizes());
            $("#pgSub").textContent = this.t("run.sub.left", { time: dur(this.L, left) });
        }
        this.syncTabs();
    }
    // ---------- zakładki: Zainstaluj, Systemy, Odinstaluj Shrimp ----------
    /** Zaznaczona zakładka według ekranu; w trakcie przygotowania nośnika pozostałe są nieaktywne. */
    syncTabs() {
        const cur = this.screen === "systemy" ? "systems" : this.screen === "odinstaluj" ? "uninstall" : "install";
        $$(".tab").forEach((t) => {
            t.setAttribute("aria-selected", String(t.dataset.tab === cur));
            t.disabled = this.running && t.dataset.tab !== "install";
        });
    }
    tab(name) {
        if (this.running && name !== "install")
            return;
        if (name === "systems")
            this.go("systemy");
        else if (name === "uninstall")
            this.go("odinstaluj");
        else
            this.go(this.installScreen || "start");
        if (name !== "install")
            this.loadSystems();
    }
    /** Stan systemów z UEFI (Shrimp zainstalowany, domyślny system) dla obu zakładek. */
    async loadSystems() {
        $("#sysErr").hidden = true;
        let s = null;
        try {
            s = await this.be.systems();
        }
        catch (e) {
            this.showErr("sysErr", "sys.fail", e);
        }
        this.sys = s;
        const has = !!s?.shrimp;
        $("#sysNone").hidden = has;
        $("#sysShrimp").hidden = !has;
        $("#sysFw").hidden = s?.firmware !== "uefi";
        $$("#sysDefault .mode").forEach((b) => b.setAttribute("aria-checked", String(b.dataset.sys === s?.default)));
        $("#unNone").hidden = has || !$("#unDone").hidden;
        $("#unBtn").disabled = !has;
    }
    showErr(id, key, e) {
        $(`#${id}`).hidden = false;
        $(`#${id}Txt`).innerHTML = `<b>${esc(this.t(key))}</b> ${esc(hostMessage(this.L, e?.message || String(e)))}`;
    }
    /** Okno potwierdzenia: `yes` działa po „tak”, okno zamyka się po jego zakończeniu. */
    confirm(title, text, label, yes) {
        $("#cfT").textContent = this.t(title);
        $("#cfD").textContent = this.t(text);
        $("#cfYes").textContent = this.t(label);
        this.onConfirm = yes;
        $("#confirm").hidden = false;
        $("#cfNo").focus();
    }
    restartWith(action, errId) {
        this.confirm("sys.confirm.t", "sys.confirm.d", "sys.restart.btn", async () => {
            try {
                await action();
            }
            catch (e) {
                this.showErr(errId, "sys.fail", e);
            }
        });
    }
    startRun() {
        this.cur = null;
        this.finished = false;
        this.failed = null;
        this.running = true;
        this.drawRun();
        const d = this.device;
        this.be.start({ device: d.id, folders: this.mode === "auto" ? this.folders.filter((f) => f.on).map((f) => f.key) : [], paths: this.mode === "manual" ? this.picked.map((p) => p.path) : [] }, {
            progress: (p) => { if (!this.running)
                return; this.cur = p; this.drawRun(); },
            image: (size) => { this.info.shrimpSize = size; this.drawRun(); },
            fail: (f) => { this.running = false; this.failed = f; this.drawRun(); },
            done: () => { this.running = false; this.finished = true; this.drawRun(); if (!this.preview)
                this.go("gotowe"); },
        });
    }
    // ---------- strony pomocy ----------
    /** Ikony z data-help otwierają stronę pomocy, gdy jest adres (boxes/help-links/help.json albo manifest); inaczej zostaje sama podpowiedź. */
    initHelp() {
        $$("[data-help]").forEach((el) => {
            const url = helpUrl(el.dataset.help, this.L.lang, "install", this.info.helpUrl);
            el.classList.toggle("help", !!url);
            if (url)
                el.dataset.url = url;
            else
                delete el.dataset.url;
        });
    }
    openHelp(el) {
        const url = el?.dataset.url;
        if (url)
            this.be.openUrl(url);
    }
    // ---------- dialog ----------
    openDlg() { $("#scrim").hidden = false; $("#dlgNo").focus(); }
    closeDlg() { $("#scrim").hidden = true; $("#cancelBtn")?.focus(); this.closeAfterCancel = false; }
    // ---------- nawigacja ----------
    go(r) {
        const ROUTES = {
            start: ["start"], nosnik: ["nosnik"], "nosnik-lista": ["nosnik", "", "list"], "nosnik-info": ["nosnik", "", "tip"],
            "brak-nosnika": ["nosnik"], "za-maly": ["nosnik"],
            pliki: ["pliki", "auto"], "pliki-reczny": ["pliki", "manual"], postep: ["postep"], "brak-internetu": ["postep"], "nosnik-zajety": ["postep"], "pytanie-rufusa": ["postep"], gotowe: ["gotowe"],
            systemy: ["systemy"], odinstaluj: ["odinstaluj"],
        };
        const [scr, variant, flag] = ROUTES[r] || ROUTES.start;
        this.screen = scr;
        if (scr !== "systemy" && scr !== "odinstaluj")
            this.installScreen = r;
        this.syncTabs();
        $$(".screen").forEach((s) => (s.hidden = s.dataset.screen !== scr));
        const s = $(`.screen[data-screen="${scr}"]`);
        this.closePop(false);
        $("#hint").classList.toggle("show", flag === "tip");
        const lg = s.querySelector(".sh");
        if (lg)
            playIntro(lg);
        if (scr === "nosnik") {
            this.drawCb();
            if (flag === "list")
                requestAnimationFrame(() => { this.openPop(); $("#cb").focus(); });
        }
        if (scr === "pliki")
            this.setMode(variant || this.mode, false);
        if (scr === "postep" && !this.running && !this.finished && !this.failed)
            this.startRun();
        else if (scr === "postep")
            this.drawRun();
        $("#scrim").hidden = true;
        const scrEl = $(".scr", s);
        if (scrEl)
            scrEl.scrollTop = 0;
        riseScreen(s);
    }
    wire() {
        document.addEventListener("click", (e) => {
            const g = e.target.closest("[data-go]");
            if (g && !g.disabled) {
                e.preventDefault();
                this.go(g.dataset.go);
            }
        });
        document.addEventListener("click", (e) => this.openHelp(e.target.closest("[data-help].help")));
        document.addEventListener("keydown", (e) => {
            if (e.key === "Enter" && e.target.matches?.("[data-help].help"))
                this.openHelp(e.target);
        });
        // pasek tytułu: przeciąganie i przyciski okna
        $(".titlebar").addEventListener("mousedown", (e) => { if (e.button === 0 && !e.target.closest(".caps"))
            this.be.drag(); });
        $("#minBtn").addEventListener("click", () => this.be.minimize());
        $("#closeBtn").addEventListener("click", () => {
            if (this.running) {
                this.closeAfterCancel = true;
                this.openDlg();
            }
            else
                this.be.close();
        });
        // nośnik
        const cb = $("#cb"), pop = $("#cbPop");
        cb.addEventListener("click", () => (pop.hidden ? this.openPop() : this.closePop(true)));
        cb.addEventListener("keydown", (e) => { if (e.key === "ArrowDown" || e.key === "ArrowUp") {
            e.preventDefault();
            this.openPop();
        } });
        pop.addEventListener("click", (e) => {
            const o = e.target.closest(".opt");
            if (!o || o.getAttribute("aria-disabled"))
                return;
            this.sel = +o.dataset.id;
            if (!this.touched)
                this.defaults();
            this.drawCb();
            this.closePop(true);
        });
        pop.addEventListener("keydown", (e) => {
            const opts = $$(".opt:not([aria-disabled])", pop), i = opts.indexOf(document.activeElement);
            if (e.key === "ArrowDown") {
                e.preventDefault();
                (opts[i + 1] || opts[0]).focus();
            }
            else if (e.key === "ArrowUp") {
                e.preventDefault();
                (opts[i - 1] || opts[opts.length - 1]).focus();
            }
            else if (e.key === "Escape") {
                e.preventDefault();
                this.closePop(true);
            }
            else if (e.key === "Tab")
                this.closePop(false);
        });
        document.addEventListener("mousedown", (e) => {
            const t = e.target;
            if (!pop.hidden && !t.closest("#cbPop") && !t.closest("#cb"))
                this.closePop(false);
        });
        // pliki
        $$(".mode").forEach((b) => b.addEventListener("click", () => {
            if (b.getAttribute("aria-checked") !== "true")
                this.setMode(b.dataset.mode, true);
        }));
        $("#folders").addEventListener("click", (e) => {
            const t = e.target.closest(".fol");
            if (!t || t.disabled)
                return;
            const f = this.folders[+t.dataset.i];
            f.on = !f.on;
            this.touched = true;
            this.drawFolders();
            $(`.fol[data-i="${t.dataset.i}"]`)?.focus();
        });
        $("#addFolder").addEventListener("click", () => this.add("folder"));
        $("#addFile").addEventListener("click", () => this.add("file"));
        $("#picked").addEventListener("click", (e) => {
            const r = e.target.closest("[data-rm]");
            if (!r)
                return;
            this.picked.splice(+r.dataset.rm, 1);
            this.drawPicked();
        });
        // postęp
        $("#cancelBtn").addEventListener("click", () => this.openDlg());
        $("#dlgNo").addEventListener("click", () => this.closeDlg());
        $("#dlgYes").addEventListener("click", async () => {
            const close = this.closeAfterCancel;
            $("#dlgYes").disabled = true;
            this.running = false;
            await this.be.cancel();
            $("#dlgYes").disabled = false;
            this.cur = null;
            this.failed = null;
            this.finished = false;
            if (close)
                this.be.close();
            else
                this.go("nosnik");
        });
        $("#retryBtn").addEventListener("click", () => { this.failed = null; this.startRun(); riseScreen($('.screen[data-screen="postep"]')); });
        document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !$("#scrim").hidden)
            this.closeDlg(); });
        // gotowe
        $("#laterBtn").addEventListener("click", () => this.be.later());
        $("#rebootBtn").addEventListener("click", async () => {
            const b = $("#rebootBtn");
            b.disabled = true;
            try {
                await this.be.reboot();
            }
            catch (e) {
                $("#doneInfo").hidden = true;
                $("#rebootErr").hidden = false;
                $("#rebootErrTxt").innerHTML = `<b>${esc(this.t("reboot.fail"))}</b> ${esc(hostMessage(this.L, e.message))}`;
                b.disabled = false;
            }
        });
        // zakładki
        $(".tabs").addEventListener("click", (e) => {
            const t = e.target.closest(".tab");
            if (t && !t.disabled)
                this.tab(t.dataset.tab);
        });
        // systemy
        $("#sysRestart").addEventListener("click", () => this.restartWith(() => this.be.bootShrimp(), "sysErr"));
        $("#sysFirmware").addEventListener("click", () => this.restartWith(() => this.be.firmwareReboot(), "sysErr"));
        $("#sysDefault").addEventListener("click", async (e) => {
            const b = e.target.closest(".mode");
            if (!b || b.getAttribute("aria-checked") === "true")
                return;
            $("#sysErr").hidden = true;
            try {
                await this.be.setDefault(b.dataset.sys);
            }
            catch (err) {
                this.showErr("sysErr", "sys.fail", err);
            }
            this.loadSystems();
        });
        // odinstaluj
        $("#unDisks").addEventListener("click", () => this.be.openDisks());
        $("#unBtn").addEventListener("click", () => this.confirm("un.confirm.t", "un.confirm.d", "un.btn", async () => {
            $("#unErr").hidden = true;
            try {
                await this.be.uninstall();
                $("#unDone").hidden = false;
            }
            catch (e) {
                this.showErr("unErr", "un.fail", e);
            }
            this.loadSystems();
        }));
        // okno potwierdzenia
        $("#cfNo").addEventListener("click", () => { $("#confirm").hidden = true; });
        $("#cfYes").addEventListener("click", async () => {
            const b = $("#cfYes");
            b.disabled = true;
            await this.onConfirm?.();
            b.disabled = false;
            $("#confirm").hidden = true;
        });
        document.addEventListener("keydown", (e) => { if (e.key === "Escape" && !$("#confirm").hidden)
            $("#confirm").hidden = true; });
        // logo: mrugnięcie po kliknięciu
        $$(".sh").forEach((lg) => {
            lg.addEventListener("click", () => { lg.classList.remove("go"); void lg.getBoundingClientRect(); lg.classList.add("go"); });
            $(".lid", lg)?.addEventListener("animationend", (e) => { if (e.animationName === "sh-wink-once")
                lg.classList.remove("go"); });
        });
    }
    /** Podgląd: stan ręcznego wyboru z makiety. */
    presetPicked(p) { this.picked = p.slice(); }
}
function playIntro(lg) {
    lg.classList.remove("play", "go");
    void lg.getBoundingClientRect();
    lg.classList.add("play");
}
/** Wejście elementów ekranu: unoszą się i powiększają kolejno co 70 ms. */
function rise(els) {
    els.forEach((el, i) => {
        el.classList.remove("rise");
        el.style.setProperty("--i", String(i));
        void el.offsetWidth;
        el.classList.add("rise");
    });
}
function riseScreen(s) {
    const els = s.querySelector(".start-scr")
        ? $$("h1, .sub, .feat > div, .cmd > *", s)
        : $$(".scr > *, .cmd > *", s).filter((el) => !el.hidden);
    rise(els);
}
