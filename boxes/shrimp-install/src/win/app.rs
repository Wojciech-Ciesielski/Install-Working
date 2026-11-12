//! Przebieg przygotowania nośnika i most do interfejsu.
//!
//! Interfejs wysyła tekst `id⇥wywołanie⇥argumenty…`, program odpowiada `{ id, ok, result | error }`
//! albo zdarzeniem `{ event, … }` (umowa: ui/src/backend/types.ts, opis w README).
use super::{boot, collect, disk, fetch, sys};
use crate::backup;
use crate::ctx::{cancelled, fail, Ctx, R};
use crate::manifest::{self, Manifest};
use crate::transfer::{self, Item};
use crate::tree;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// Najmniejszy nośnik: pendrive „16 GB” ma naprawdę 14,5–16 mld bajtów; interfejs pisze „16 GB”.
pub const MIN_MEDIA: u64 = 14_000_000_000;
/// Szacunek rozmiaru obrazu, zanim przyjdzie manifest.
const ISO_ESTIMATE: u64 = 2000 << 20;
const GIB: u64 = 1 << 30;
const FAT32_MAX: u64 = 4 * GIB - 1;

#[derive(Default, Clone)]
pub struct Opts {
    pub demo: bool,
    pub screen: Option<String>,
    pub theme: Option<String>,
    pub lang: Option<String>,
    pub source: Option<PathBuf>,
    pub manifest: Option<String>,
    pub collect: Option<PathBuf>,
    pub auto_disk: Option<u32>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum State {
    Idle,
    Busy,
    Done,
    Failed,
}

/// Wywołania, które obsługuje okno (wątek interfejsu).
pub enum Action {
    None,
    Close,
    Minimize,
    Drag,
    Pick { id: u64, folder: bool },
}

pub struct App {
    pub ctx: Arc<Ctx>,
    pub opts: Opts,
    state: Mutex<State>,
    disk: Mutex<Option<u32>>,
    devices: Mutex<Option<Vec<disk::Disk>>>,
    folders: Mutex<Vec<(String, PathBuf, Option<u64>)>>,
    folders_started: AtomicBool,
    pub bitlocker: AtomicBool,
    manifest: Mutex<Option<Manifest>>,
    cancel_id: Mutex<Option<u64>>,
}

impl App {
    pub fn new(ctx: Arc<Ctx>, opts: Opts) -> Arc<App> {
        let folders = sys::folders().into_iter().map(|(k, p)| (k, p, None)).collect();
        Arc::new(App {
            ctx,
            opts,
            state: Mutex::new(State::Idle),
            disk: Mutex::new(None),
            devices: Mutex::new(None),
            folders: Mutex::new(folders),
            folders_started: AtomicBool::new(false),
            bitlocker: AtomicBool::new(false),
            manifest: Mutex::new(None),
            cancel_id: Mutex::new(None),
        })
    }

    fn manifest_url(&self) -> String {
        self.opts.manifest.clone().unwrap_or_else(|| crate::config::MANIFEST_URL.to_string())
    }

    pub fn busy(&self) -> bool {
        *self.state.lock().unwrap() == State::Busy
    }

    fn reply(&self, id: u64, v: Value) {
        if id > 0 {
            self.ctx.emit(json!({ "id": id, "ok": true, "result": v }).to_string());
        }
    }

    fn reply_err(&self, id: u64, msg: &str) {
        if id > 0 {
            self.ctx.emit(json!({ "id": id, "ok": false, "error": msg }).to_string());
        }
    }

    pub fn folders_json(&self) -> Value {
        Value::Array(self.folders.lock().unwrap().iter().map(|(k, p, s)| json!({ "key": k, "path": p, "size": s })).collect())
    }

    // ---------- tło: BitLocker, manifest, nośniki, rozmiary folderów ----------
    pub fn start_background(self: &Arc<Self>) {
        let a = self.clone();
        std::thread::spawn(move || {
            let on = disk::bitlocker_on();
            crate::log!("BitLocker {}", if on { "włączony" } else { "wyłączony albo nieznany" });
            a.bitlocker.store(on, Ordering::Relaxed);
            if on {
                a.ctx.emit(r#"{"event":"bitlocker","on":true}"#.into());
            }
        });
        if self.opts.source.is_none() {
            let a = self.clone();
            std::thread::spawn(move || match fetch::manifest(&a.manifest_url()) {
                Ok(m) => *a.manifest.lock().unwrap() = Some(m),
                Err(f) => crate::log!("manifest przy starcie: {f:?}"),
            });
        }
        let a = self.clone();
        std::thread::spawn(move || loop {
            a.refresh_devices();
            std::thread::sleep(Duration::from_secs(3));
        });
    }

    fn refresh_devices(&self) -> Option<Vec<disk::Disk>> {
        match disk::list(self.opts.auto_disk) {
            Ok(list) => {
                let mut cur = self.devices.lock().unwrap();
                if cur.as_ref() != Some(&list) {
                    if cur.is_some() {
                        let d: Vec<Value> = list.iter().map(disk::Disk::to_json).collect();
                        self.ctx.emit(json!({ "event": "devices", "devices": d }).to_string());
                    }
                    *cur = Some(list.clone());
                }
                Some(list)
            }
            Err(e) => {
                crate::log!("lista nośników: {e}");
                None
            }
        }
    }

    fn start_folder_sizes(self: &Arc<Self>) {
        if self.folders_started.swap(true, Ordering::Relaxed) {
            return;
        }
        let a = self.clone();
        std::thread::spawn(move || {
            let n = a.folders.lock().unwrap().len();
            for i in 0..n {
                let p = a.folders.lock().unwrap()[i].1.clone();
                let s = tree::size(&tree::long(&p), None);
                a.folders.lock().unwrap()[i].2 = Some(s);
                a.ctx.emit(json!({ "event": "folders", "folders": a.folders_json() }).to_string());
            }
        });
    }

    // ---------- wywołania z interfejsu ----------
    pub fn dispatch(self: &Arc<Self>, msg: &str) -> Action {
        let f: Vec<&str> = msg.split('\t').collect();
        let id: u64 = f.first().and_then(|s| s.parse().ok()).unwrap_or(0);
        let call = f.get(1).copied().unwrap_or("");
        if matches!(call, "start" | "cancel" | "reboot") {
            crate::log!("wywołanie {call} (stan {:?})", *self.state.lock().unwrap());
        }
        match call {
            "info" => {
                let size = self.manifest.lock().unwrap().as_ref().map_or(ISO_ESTIMATE, |m| m.size);
                let help = self.manifest.lock().unwrap().as_ref().and_then(|m| m.help_url.clone()).filter(|u| manifest::safe_url(u));
                let mut v = json!({ "bitlocker": self.bitlocker.load(Ordering::Relaxed), "shrimpSize": size, "minSize": MIN_MEDIA, "folders": self.folders_json() });
                if let Some(h) = help {
                    v["helpUrl"] = json!(h);
                }
                self.reply(id, v);
                self.start_folder_sizes();
            }
            "devices" => {
                let cached = self.devices.lock().unwrap().clone();
                match cached {
                    Some(l) => self.reply(id, Value::Array(l.iter().map(disk::Disk::to_json).collect())),
                    None => {
                        let a = self.clone();
                        std::thread::spawn(move || {
                            let l = a.refresh_devices().unwrap_or_default();
                            a.reply(id, Value::Array(l.iter().map(disk::Disk::to_json).collect()));
                        });
                    }
                }
            }
            "pick" => return Action::Pick { id, folder: f.get(2) == Some(&"folder") },
            "start" => self.begin(id, &f),
            "cancel" => {
                if self.busy() {
                    *self.cancel_id.lock().unwrap() = Some(id);
                    self.ctx.cancel.store(true, Ordering::Relaxed);
                } else {
                    *self.state.lock().unwrap() = State::Idle;
                    self.reply(id, Value::Null);
                }
            }
            "reboot" => {
                let a = self.clone();
                let disk = *self.disk.lock().unwrap();
                std::thread::spawn(move || match disk.ok_or("noMedia").and_then(boot::set_boot_next) {
                    Err(e) => a.reply_err(id, e),
                    Ok(()) => {
                        boot::disable_fast_startup();
                        if boot::reboot() { a.reply(id, Value::Null) } else { a.reply_err(id, "rebootFail") }
                    }
                });
            }
            "open" => {
                if let Some(u) = f.get(2).filter(|u| manifest::safe_url(u)) {
                    open_url(u);
                }
            }
            "later" | "close" => return Action::Close,
            "minimize" => return Action::Minimize,
            "drag" => return Action::Drag,
            _ => self.reply_err(id, "unknown"),
        }
        Action::None
    }

    /// Wynik okna wyboru: rozmiary liczone w tle.
    pub fn picked(self: &Arc<Self>, id: u64, folder: bool, paths: Vec<PathBuf>) {
        let a = self.clone();
        std::thread::spawn(move || {
            let v: Vec<Value> = paths
                .iter()
                .map(|p| {
                    let name = p.file_name().map_or_else(|| p.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned());
                    json!({ "path": p, "name": name, "kind": if folder { "folder" } else { "file" }, "size": tree::size(&tree::long(p), None) })
                })
                .collect();
            a.reply(id, Value::Array(v));
        });
    }

    fn begin(self: &Arc<Self>, id: u64, f: &[&str]) {
        if self.busy() {
            return self.reply_err(id, "busy");
        }
        let Some(n) = f.get(2).and_then(|s| s.parse::<u32>().ok()) else { return self.reply_err(id, "device") };
        // Tylko nośnik z listy (albo dysk testowy z --auto-disk): interfejs nie może wskazać dysku z Windows.
        let known = self.devices.lock().unwrap().as_ref().is_some_and(|l| l.iter().any(|d| d.id == n));
        if !known && self.opts.auto_disk != Some(n) {
            return self.reply_err(id, "device");
        }
        let keys: Vec<&str> = f.get(3).map_or(vec![], |k| k.split(',').filter(|k| !k.is_empty()).collect());
        let paths: Vec<String> = f.iter().skip(4).map(|s| s.to_string()).collect();
        let folders: Vec<(String, PathBuf)> = self.folders.lock().unwrap().iter().map(|(k, p, _)| (k.clone(), p.clone())).collect();
        let items = transfer::plan(&folders, &keys, &paths);
        *self.disk.lock().unwrap() = Some(n);
        *self.state.lock().unwrap() = State::Busy;
        self.ctx.cancel.store(false, Ordering::Relaxed);
        self.reply(id, Value::Null);
        let a = self.clone();
        std::thread::spawn(move || {
            let r = a.run(n, &items);
            let cancel_id = a.cancel_id.lock().unwrap().take();
            match r {
                Ok(()) => {
                    *a.state.lock().unwrap() = State::Done;
                    a.ctx.emit(r#"{"event":"done"}"#.into());
                }
                Err(f) if f.kind == "cancel" || a.ctx.is_cancelled() => {
                    *a.state.lock().unwrap() = State::Idle;
                    crate::log!("anulowano w fazie {}", f.phase);
                    a.reply(cancel_id.unwrap_or(0), Value::Null);
                }
                Err(f) => {
                    *a.state.lock().unwrap() = State::Failed;
                    crate::log!("błąd w fazie {}: {} {}", f.phase, f.kind, f.message.as_deref().unwrap_or(""));
                    let mut v = json!({ "event": "fail", "phase": f.phase, "kind": f.kind });
                    if let Some(m) = f.message.filter(|m| !m.is_empty()) {
                        v["message"] = json!(m);
                    }
                    a.ctx.emit(v.to_string());
                }
            }
        });
    }

    // ---------- przebieg ----------
    fn run(&self, n: u32, items: &[Item]) -> R<()> {
        let ctx = &*self.ctx;
        // 1. Obraz
        let known = self.manifest.lock().unwrap().clone();
        let img = fetch::obtain(ctx, self.opts.source.as_deref(), &self.manifest_url(), known)?;
        ctx.check(1)?;
        crate::log!("obraz gotowy: {} ({} B)", img.path.display(), img.size);

        // 2. Nośnik: obraz zamontowany, żeby znać rozmiary, potem kopia zawartości, formatowanie i pliki obrazu.
        ctx.progress(2, Some("prepare"), 0, 0, 0);
        let d = disk::find(n).map_err(|e| {
            crate::log!("nośnik {n}: {e}");
            fail(2, "device", None)
        })?;
        let iso_path = img.path.to_string_lossy().into_owned();
        let iso = disk::mount_iso(&iso_path).map_err(|e| {
            crate::log!("montowanie obrazu: {e}");
            fail(2, "write", Some("imageBad"))
        })?;
        let _mounted = Mounted(iso_path.clone());
        let files = scan_image(&iso).map_err(|e| {
            crate::log!("obraz: {e}");
            fail(2, "write", Some("imageBad"))
        })?;
        let boot_need: u64 = files.iter().filter(|f| !f.live).map(|f| f.size).sum();
        let image_total: u64 = files.iter().map(|f| f.size).sum();
        let boot_bytes = (boot_need + boot_need / 5 + (256 << 20)).max(GIB).div_ceil(1 << 20) << 20;
        crate::log!("obraz: {} plików, {image_total} B, partycja startowa {boot_bytes} B", files.len());
        let items_bytes: u64 = items.iter().map(|i| tree::size(&tree::long(&i.source), Some(&ctx.cancel))).sum();
        if boot_bytes + image_total + items_bytes + (512 << 20) > d.size {
            return Err(fail(2, "space", Some("mediaSmall")));
        }

        // Najpierw zawartość nośnika: bez sprawdzonej kopii nic nie zostanie sformatowane.
        let vols = disk::volumes(n).map_err(|e| {
            crate::log!("woluminy nośnika: {e}");
            fail(2, "copy", Some("backupRead"))
        })?;
        let staging = staging_dir(&d);
        ctx.speed_reset(0);
        let list = backup::stage(&staging, &vols, &d.style.to_lowercase(), d.size, boot_bytes + image_total + items_bytes, sys::free_space, &ctx.cancel, &mut |done, total| {
            ctx.progress(2, Some("backup"), done, total, ctx.speed(done));
        })?;
        ctx.check(2)?;
        crate::log!("kopia zawartości nośnika gotowa ({} B), formatowanie", list.bytes);
        backup::mark_formatted(&staging);
        ctx.progress(2, Some("prepare"), 0, 0, 0);
        let (boot_root, live_root) = disk::prepare(n, boot_bytes).map_err(|e| {
            crate::log!("formatowanie: {e}");
            let l = e.to_lowercase();
            if l.contains("system_disk") {
                fail(2, "device", None)
            } else if l.contains("in use") || l.contains("used by another") || l.contains("access is denied") || l.contains("0x80070020") {
                fail(2, "busy", None)
            } else {
                fail(2, "write", Some(&e))
            }
        })?;
        crate::log!("nośnik: {} (FAT32), {} (exFAT)", boot_root.display(), live_root.display());
        copy_image(ctx, &iso, &files, &boot_root, &live_root, image_total)?;
        drop(_mounted);
        if !img.keep {
            let _ = fs::remove_file(&img.path);
        }
        ctx.progress(2, None, image_total, image_total, 0);

        // 3. Dawna zawartość nośnika (jedyna kopia), pliki z Windows, shrimp.json, język GRUB.
        let total = items_bytes + 2 * list.bytes;
        let mut done = 0u64;
        ctx.speed_reset(0);
        ctx.bytes(3, 0, total);
        backup::write_back(&staging, &list, &live_root, &ctx.cancel, &mut |k| {
            done += k;
            ctx.bytes(3, done, total);
        })?;
        let root = live_root.join(transfer::DIR);
        let copied = transfer::copy(&root, items, &ctx.cancel, &mut |k| {
            done += k;
            ctx.bytes(3, done, total);
        })?;
        let desc = transfer::description(&img.version, &img.sha256, &collect::collect(self.bitlocker.load(Ordering::Relaxed)), &copied);
        transfer::write_description(&root, &desc)?;
        let tag = self.opts.lang.clone().or_else(|| sys::ui_languages().into_iter().next()).map_or_else(|| "en".into(), |l| crate::lang::tag(&l));
        transfer::write_grub_lang(&boot_root, &tag);
        ctx.progress(3, None, total, total, 0);
        Ok(())
    }
}

/// Odmontowanie obrazu także po błędzie.
struct Mounted(String);
impl Drop for Mounted {
    fn drop(&mut self) {
        disk::dismount_iso(&self.0);
    }
}

pub struct ImageFile {
    pub rel: PathBuf,
    pub size: u64,
    pub dir: bool,
    /// LiveOS/… idzie na exFAT, reszta na FAT32.
    pub live: bool,
}

/// Lista plików obrazu. Obraz musi mieć LiveOS\squashfs.img i program startowy UEFI.
pub fn scan_image(root: &Path) -> Result<Vec<ImageFile>, String> {
    fn walk(root: &Path, rel: &Path, out: &mut Vec<ImageFile>) -> std::io::Result<()> {
        let mut items: Vec<_> = fs::read_dir(root.join(rel))?.collect::<std::io::Result<_>>()?;
        items.sort_by_key(|e| e.file_name());
        for e in items {
            let r = rel.join(e.file_name());
            let live = r.components().next().is_some_and(|c| c.as_os_str().eq_ignore_ascii_case("LiveOS"));
            let m = e.metadata()?;
            out.push(ImageFile { rel: r.clone(), size: if m.is_dir() { 0 } else { m.len() }, dir: m.is_dir(), live });
            if m.is_dir() {
                walk(root, &r, out)?;
            }
        }
        Ok(())
    }
    let mut v = Vec::new();
    walk(root, Path::new(""), &mut v).map_err(|e| e.to_string())?;
    let has = |p: &str| v.iter().any(|f| f.rel.to_string_lossy().replace('/', "\\").eq_ignore_ascii_case(p));
    if !has(r"LiveOS\squashfs.img") || !has(r"EFI\BOOT\bootx64.efi") {
        return Err("brak LiveOS\\squashfs.img albo EFI\\BOOT\\bootx64.efi".into());
    }
    if let Some(f) = v.iter().find(|f| !f.live && f.size > FAT32_MAX) {
        return Err(format!("{} > 4 GB poza LiveOS", f.rel.display()));
    }
    Ok(v)
}

fn copy_image(ctx: &Ctx, iso: &Path, files: &[ImageFile], boot: &Path, live: &Path, total: u64) -> R<()> {
    let mut buf = vec![0u8; tree::BUF];
    let mut done = 0u64;
    ctx.speed_reset(0);
    for f in files {
        let dst = if f.live { live } else { boot }.join(&f.rel);
        if f.dir {
            fs::create_dir_all(&dst).map_err(|e| tree::media_error(2, &e).unwrap_or(fail(2, "write", None)))?;
            continue;
        }
        match tree::copy_file(&iso.join(&f.rel), &dst, &mut buf, &ctx.cancel, &mut |k| {
            done += k;
            ctx.bytes(2, done, total);
        }) {
            Ok(_) => {}
            Err(tree::CopyErr::Cancel) => return Err(cancelled(2)),
            Err(tree::CopyErr::Read(e)) => {
                crate::log!("odczyt obrazu {}: {e}", f.rel.display());
                return Err(fail(2, "write", Some("imageBad")));
            }
            Err(tree::CopyErr::Write(e)) => {
                crate::log!("zapis {}: {e}", dst.display());
                return Err(tree::media_error(2, &e).unwrap_or(fail(2, "write", None)));
            }
        }
    }
    Ok(())
}

/// %LOCALAPPDATA%\Shrimp\usb-backup\<16 znaków sumy identyfikatora nośnika>
fn staging_dir(d: &disk::Disk) -> PathBuf {
    sys::local_app_data().join("Shrimp").join("usb-backup").join(&manifest::sha256(d.ident().as_bytes())[..16])
}

fn open_url(url: &str) {
    use windows::core::{w, HSTRING};
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(url), None, None, SW_SHOWNORMAL);
    }
}
