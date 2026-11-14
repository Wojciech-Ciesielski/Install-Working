// Most do programu Shrimp Center (WebView2). Wiadomości do programu to tekst rozdzielony
// tabulatorami: „id⇥wywołanie⇥argumenty…” (ścieżki Windows nie mogą zawierać tabulatora).
// Program odpowiada obiektem JSON: { id, ok, result | error } albo zdarzeniem { event, … }, wywołując
// window.__shrimpRecv(obiekt) (program w Rust, wry); zdarzenie „message” WebView2 też jest obsługiwane.
export const hasHost = () => !!window.chrome?.webview;
export class HostBackend {
    wv = window.chrome.webview;
    seq = 0;
    wait = new Map();
    devices = [];
    folders = [];
    lastDevices = null;
    lastFolders = null;
    run = null;
    bitlocker = [];
    closeReq = [];
    constructor() {
        this.wv.addEventListener("message", (e) => this.recv(e.data));
        window.__shrimpRecv = (m) => this.recv(m);
    }
    recv(m) {
        if (m.id !== undefined) {
            const w = this.wait.get(m.id);
            if (!w)
                return;
            this.wait.delete(m.id);
            if (m.ok)
                w.ok(m.result);
            else
                w.err(new Error(m.error || "Błąd"));
            return;
        }
        switch (m.event) {
            case "devices":
                this.lastDevices = m.devices;
                this.devices.forEach((cb) => cb(m.devices));
                break;
            case "folders":
                this.lastFolders = m.folders;
                this.folders.forEach((cb) => cb(m.folders));
                break;
            case "progress":
                this.run?.progress(m);
                break;
            case "image":
                if (m.size > 0)
                    this.run?.image?.(m.size);
                break;
            case "fail":
                this.run?.fail(m);
                break;
            case "done":
                this.run?.done();
                break;
            case "bitlocker":
                this.bitlocker.forEach((cb) => cb(!!m.on));
                break;
            case "closeRequested":
                this.closeReq.forEach((cb) => cb());
                break;
        }
    }
    call(name, ...args) {
        const id = ++this.seq;
        return new Promise((ok, err) => {
            this.wait.set(id, { ok, err });
            this.wv.postMessage([id, name, ...args].join("\t"));
        });
    }
    send(name) { this.wv.postMessage([0, name].join("\t")); }
    info() { return this.call("info"); }
    onDevices(cb) {
        this.devices.push(cb);
        if (this.lastDevices)
            cb(this.lastDevices);
        else
            this.call("devices").then((d) => { this.lastDevices = d; cb(d); });
    }
    onFolders(cb) {
        this.folders.push(cb);
        if (this.lastFolders)
            cb(this.lastFolders);
    }
    onBitLocker(cb) { this.bitlocker.push(cb); }
    onCloseRequest(cb) { this.closeReq.push(cb); }
    pick(kind) { return this.call("pick", kind); }
    start(plan, ev) {
        this.run = ev;
        this.call("start", plan.device, plan.folders.join(","), ...plan.paths).catch((e) => ev.fail({ phase: 1, kind: "other", message: e.message }));
    }
    async cancel() { await this.call("cancel"); this.run = null; }
    async reboot() { await this.call("reboot"); }
    // Zakładki Systemy i Odinstaluj Shrimp (src/win/systems.rs).
    systems() { return this.call("systems"); }
    async bootShrimp() { await this.call("bootnext"); }
    async setDefault(which) { await this.call("default", which); }
    async firmwareReboot() { await this.call("firmware"); }
    uninstall() { return this.call("uninstall"); }
    openDisks() { this.send("disks"); }
    openUrl(url) { this.wv.postMessage([0, "open", url].join("\t")); }
    later() { this.send("later"); }
    minimize() { this.send("minimize"); }
    close() { this.send("close"); }
    drag() { this.send("drag"); }
}
