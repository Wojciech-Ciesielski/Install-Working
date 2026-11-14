// Dane przykładowe z makiety: podgląd w przeglądarce, zrzuty ekranów i `shrimp-center.exe --demo`.
import { GiB } from "../logic.js";
const MB = 1024 ** 2;
export const DEMO_DEVICES = [
    { id: 1, name: "SanDisk Ultra (E:)", kind: "usb", size: 29.8 * GiB },
    { id: 2, name: "Samsung EVO Select (G:)", kind: "sd", size: 59.6 * GiB },
    { id: 3, name: "WD Elements (H:)", kind: "disk", size: 931.5 * GiB },
    { id: 4, name: "Kingston DataTraveler (F:)", kind: "usb", size: 7.5 * GiB },
];
const FOLDERS = [
    { key: "documents", path: "C:\\Users\\Jan\\Documents", size: 3.2 * GiB },
    { key: "pictures", path: "C:\\Users\\Jan\\Pictures", size: 8.9 * GiB },
    { key: "desktop", path: "C:\\Users\\Jan\\Desktop", size: 0.4 * GiB },
    { key: "music", path: "C:\\Users\\Jan\\Music", size: 5.1 * GiB },
    { key: "videos", path: "C:\\Users\\Jan\\Videos", size: 14.7 * GiB },
    { key: "downloads", path: "C:\\Users\\Jan\\Downloads", size: 2.0 * GiB },
];
const POOL = [
    { name: "Rodzina 2024", kind: "folder", size: 4.8 * GiB },
    { name: "Umowa najmu.pdf", kind: "file", size: 2 * MB },
    { name: "Projekty", kind: "folder", size: 2.6 * GiB },
    { name: "Praca dyplomowa.docx", kind: "file", size: 10 * MB },
    { name: "Skany dokumentów", kind: "folder", size: 1.3 * GiB },
    { name: "Podatki", kind: "folder", size: 0.2 * GiB },
].map((p) => ({ ...p, path: "C:\\Users\\Jan\\" + p.name }));
export class DemoBackend {
    o;
    timer = 0;
    pool = POOL.slice(2);
    constructor(o = {}) {
        this.o = o;
        this.sysState = { firmware: "uefi", shrimp: { id: "0002", name: "Linux Boot Manager" }, default: "windows" };
    }
    /** Dwa pierwsze elementy wyboru ręcznego, jak w makiecie. */
    static PICKED = POOL.slice(0, 2);
    async info() {
        return { bitlocker: false, shrimpSize: 1.94 * GiB, minSize: 14e9, folders: FOLDERS };
    }
    onDevices(cb) {
        const v = this.o.devices || "ok";
        cb(v === "ok" ? DEMO_DEVICES : v === "small" ? [DEMO_DEVICES[3]] : []);
    }
    onFolders(cb) { cb(FOLDERS); }
    onBitLocker(_cb) { }
    onCloseRequest(_cb) { }
    async pick(kind) {
        const i = this.pool.findIndex((p) => p.kind === kind);
        return i < 0 ? [] : this.pool.splice(i, 1);
    }
    start(plan, ev) {
        const files = plan.paths.length
            ? POOL.filter((p) => plan.paths.includes(p.path)).reduce((a, p) => a + p.size, 0)
            : FOLDERS.filter((f) => plan.folders.includes(f.key)).reduce((a, f) => a + (f.size || 0), 0);
        const shrimp = 1.94 * GiB;
        // Całość 0–100: pobieranie 0–50, zapis 50–70, kopiowanie 70–100 (jak w makiecie).
        const PH = [0, 50, 70, 100], SPEED = [0, 14 * 1024 ** 2, 38 * 1024 ** 2, 52 * 1024 ** 2];
        const sizes = [0, shrimp, shrimp, files];
        let total = this.o.at ?? 0;
        const tick = () => {
            if (this.o.fail) {
                ev.progress({ phase: 2, done: 0.02 * shrimp, total: shrimp, speed: 0 });
                ev.fail(this.o.fail);
                return false;
            }
            if (this.o.offline) {
                ev.progress({ phase: 1, done: 0.06 * 2 * shrimp, total: shrimp, speed: 0 });
                ev.fail({ phase: 1, kind: "offline" });
                return false;
            }
            if (total >= 100) {
                ev.done();
                return false;
            }
            const ph = (total >= PH[2] ? 3 : total >= PH[1] ? 2 : 1);
            const f = (total - PH[ph - 1]) / (PH[ph] - PH[ph - 1]);
            const wob = 1 + Math.sin(total / 3) * 0.06;
            ev.progress({ phase: ph, done: f * sizes[ph], total: sizes[ph], speed: SPEED[ph] * wob });
            return true;
        };
        clearInterval(this.timer);
        if (!tick() || this.o.frozen)
            return;
        this.timer = window.setInterval(() => { total = Math.min(100, total + 0.9); if (!tick())
            clearInterval(this.timer); }, 220);
    }
    async cancel() { clearInterval(this.timer); }
    async reboot() { }
    // Systemy: Shrimp obok Windows, domyślnie Windows (?screen=systemy, ?screen=odinstaluj).
    systems() { return Promise.resolve({ ...this.sysState }); }
    async bootShrimp() { }
    async setDefault(which) { this.sysState.default = which; }
    async firmwareReboot() { }
    async uninstall() { this.sysState = { firmware: "uefi", shrimp: null, default: "windows" }; return { removed: ["Boot0002", "EFI\\systemd", "loader"] }; }
    openDisks() { }
    openUrl(url) { if (/^https:\/\//.test(url))
        window.open(url, "_blank", "noopener"); }
    later() { }
    minimize() { }
    close() { }
    drag() { }
}
