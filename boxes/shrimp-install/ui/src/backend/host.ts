// Most do programu Shrimp Install (WebView2). Wiadomości do programu to tekst rozdzielony
// tabulatorami: „id⇥wywołanie⇥argumenty…” (ścieżki Windows nie mogą zawierać tabulatora).
// Program odpowiada obiektem JSON: { id, ok, result | error } albo zdarzeniem { event, … }.
import type { Backend, Device, Folder, Info, Picked, Plan, RunEvents } from "./types";

interface WebView {
  postMessage(m: string): void;
  addEventListener(t: "message", cb: (e: { data: any }) => void): void;
}
declare global { interface Window { chrome?: { webview?: WebView } } }

export const hasHost = () => !!window.chrome?.webview;

export class HostBackend implements Backend {
  private wv = window.chrome!.webview!;
  private seq = 0;
  private wait = new Map<number, { ok: (v: any) => void; err: (e: Error) => void }>();
  private devices: ((d: Device[]) => void)[] = [];
  private folders: ((f: Folder[]) => void)[] = [];
  private lastDevices: Device[] | null = null;
  private lastFolders: Folder[] | null = null;
  private run: RunEvents | null = null;
  private bitlocker: ((on: boolean) => void)[] = [];
  private closeReq: (() => void)[] = [];

  constructor() {
    this.wv.addEventListener("message", (e) => this.recv(e.data));
  }

  private recv(m: any) {
    if (m.id !== undefined) {
      const w = this.wait.get(m.id);
      if (!w) return;
      this.wait.delete(m.id);
      if (m.ok) w.ok(m.result);
      else w.err(new Error(m.error || "Błąd"));
      return;
    }
    switch (m.event) {
      case "devices": this.lastDevices = m.devices; this.devices.forEach((cb) => cb(m.devices)); break;
      case "folders": this.lastFolders = m.folders; this.folders.forEach((cb) => cb(m.folders)); break;
      case "progress": this.run?.progress(m); break;
      case "image": if (m.size > 0) this.run?.image?.(m.size); break;
      case "fail": this.run?.fail(m); break;
      case "done": this.run?.done(); break;
      case "bitlocker": this.bitlocker.forEach((cb) => cb(!!m.on)); break;
      case "closeRequested": this.closeReq.forEach((cb) => cb()); break;
    }
  }

  private call<T>(name: string, ...args: (string | number)[]): Promise<T> {
    const id = ++this.seq;
    return new Promise<T>((ok, err) => {
      this.wait.set(id, { ok, err });
      this.wv.postMessage([id, name, ...args].join("\t"));
    });
  }
  private send(name: string) { this.wv.postMessage([0, name].join("\t")); }

  info() { return this.call<Info>("info"); }
  onDevices(cb: (d: Device[]) => void) {
    this.devices.push(cb);
    if (this.lastDevices) cb(this.lastDevices);
    else this.call<Device[]>("devices").then((d) => { this.lastDevices = d; cb(d); });
  }
  onFolders(cb: (f: Folder[]) => void) {
    this.folders.push(cb);
    if (this.lastFolders) cb(this.lastFolders);
  }
  onBitLocker(cb: (on: boolean) => void) { this.bitlocker.push(cb); }
  onCloseRequest(cb: () => void) { this.closeReq.push(cb); }
  pick(kind: "folder" | "file") { return this.call<Picked[]>("pick", kind); }
  start(plan: Plan, ev: RunEvents) {
    this.run = ev;
    this.call("start", plan.device, plan.folders.join(","), ...plan.paths).catch((e: Error) => ev.fail({ phase: 1, kind: "other", message: e.message }));
  }
  async cancel() { await this.call("cancel"); this.run = null; }
  async reboot() { await this.call("reboot"); }
  openUrl(url: string) { this.wv.postMessage([0, "open", url].join("\t")); }
  later() { this.send("later"); }
  minimize() { this.send("minimize"); }
  close() { this.send("close"); }
  drag() { this.send("drag"); }
}
