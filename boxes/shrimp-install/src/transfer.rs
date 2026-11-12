//! Kopia plików użytkownika na nośnik i shrimp.json dla Setup.
//!
//! Na partycji exFAT (`SHRIMP_LIVE`):
//!   shrimp-transfer/shrimp.json          opis (schemat: boxes/install-schema)
//!   shrimp-transfer/files/<klucz>/…      Moje foldery: documents, pictures, desktop, music, videos, downloads
//!   shrimp-transfer/files/other/<nazwa>  Wybiorę sam
//! exFAT mieści pliki większe niż 4 GB, więc nic nie jest dzielone (`split` zostaje pusty; Setup nadal
//! skleja części ze starszych nośników FAT32).
use crate::ctx::{cancelled, fail, R};
use crate::tree::{self, CopyErr};
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub const DIR: &str = "shrimp-transfer";
pub const FOLDER_KEYS: [&str; 6] = ["documents", "pictures", "desktop", "music", "videos", "downloads"];

#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub source: PathBuf,
    /// Względem shrimp-transfer/, z „/”: files/documents, files/other/<nazwa>.
    pub dest: String,
    pub key: String,
}

/// Plan kopii: klucze „Moich folderów” (z ich ścieżkami) i ścieżki „Wybiorę sam” (bez powtórzeń nazw).
pub fn plan(folders: &[(String, PathBuf)], keys: &[&str], paths: &[String]) -> Vec<Item> {
    let mut v: Vec<Item> = Vec::new();
    for k in keys {
        if let Some((_, p)) = folders.iter().find(|(key, _)| key == k) {
            v.push(Item { source: p.clone(), dest: format!("files/{k}"), key: k.to_string() });
        }
    }
    for p in paths.iter().filter(|p| !p.is_empty()) {
        let base = p.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next().unwrap_or(p).to_string();
        let base = if base.is_empty() || base.ends_with(':') { "dysk".to_string() } else { base };
        let taken = |n: &str| v.iter().any(|i| i.dest.strip_prefix("files/other/").is_some_and(|d| d.eq_ignore_ascii_case(n)));
        let mut name = base.clone();
        let mut k = 2;
        while taken(&name) {
            name = format!("{base} ({k})");
            k += 1;
        }
        v.push(Item { source: PathBuf::from(p), dest: format!("files/other/{name}"), key: "other".into() });
    }
    v
}

pub struct Copied {
    pub files: Vec<Value>,
    pub skipped: Vec<Value>,
}

struct Run<'a> {
    done: u64,
    skipped: Vec<Value>,
    buf: Vec<u8>,
    cancel: &'a AtomicBool,
    on: &'a mut dyn FnMut(u64),
}

fn shown(p: &Path) -> String {
    let s = p.to_string_lossy();
    s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
}

impl Run<'_> {
    fn skip(&mut self, src: &Path, reason: &str) {
        self.skipped.push(json!({ "source": shown(src), "reason": reason }));
    }

    fn file(&mut self, src: &Path, dst: &Path, size: u64) -> R<()> {
        let before = self.done;
        let on = &mut *self.on;
        let mut got = 0;
        let r = tree::copy_file(src, dst, &mut self.buf, self.cancel, &mut |n| {
            got += n;
            on(n)
        });
        self.done = before + got;
        match r {
            Ok(_) => Ok(()),
            Err(CopyErr::Cancel) => Err(cancelled(3)),
            Err(CopyErr::Write(e)) => match tree::media_error(3, &e) {
                Some(f) => Err(f),
                None => Err(fail(3, "copy", None)),
            },
            Err(CopyErr::Read(e)) => {
                let _ = fs::remove_file(dst);
                crate::log!("pominięto plik ({e})");
                self.skip(src, if tree::is_locked(&e) { "locked" } else { "read" });
                // Postęp liczy cały plik, nawet pominięty.
                (self.on)(size.saturating_sub(got));
                self.done = before + size;
                Ok(())
            }
        }
    }

    fn tree(&mut self, src: &Path, dst: &Path) -> R<()> {
        if let Err(e) = fs::create_dir_all(dst) {
            return Err(tree::media_error(3, &e).unwrap_or(fail(3, "copy", None)));
        }
        let Ok(rd) = fs::read_dir(src) else {
            self.skip(src, "read");
            return Ok(());
        };
        let mut items: Vec<_> = rd.flatten().collect();
        items.sort_by_key(|e| e.file_name());
        for e in items {
            if self.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                return Err(cancelled(3));
            }
            let name = e.file_name();
            let n = name.to_string_lossy();
            if n.eq_ignore_ascii_case("desktop.ini") || n.eq_ignore_ascii_case("thumbs.db") {
                continue;
            }
            let (s, d) = (src.join(&name), dst.join(&name));
            let Ok(m) = fs::symlink_metadata(&s) else {
                self.skip(&s, "read");
                continue;
            };
            if tree::is_cloud(&m) {
                self.skip(&s, "cloud");
            } else if tree::is_link(&m) {
                // dowiązania nie są plikami użytkownika
            } else if m.is_dir() {
                self.tree(&s, &d)?;
            } else {
                self.file(&s, &d, m.len())?;
            }
        }
        Ok(())
    }
}

/// Kopiuje pozycje planu do `root` (= <nośnik>\shrimp-transfer). `on`: skopiowane bajty.
pub fn copy(root: &Path, items: &[Item], cancel: &AtomicBool, on: &mut dyn FnMut(u64)) -> R<Copied> {
    fs::create_dir_all(root.join("files")).map_err(|_| fail(3, "device", None))?;
    let mut r = Run { done: 0, skipped: Vec::new(), buf: vec![0; tree::BUF], cancel, on };
    let mut files = Vec::new();
    for it in items {
        let before = r.done;
        let src = tree::long(&it.source);
        let dst = it.dest.split('/').fold(root.to_path_buf(), |p, c| p.join(c));
        if let Some(p) = dst.parent() {
            let _ = fs::create_dir_all(p);
        }
        match fs::metadata(&src) {
            Err(_) => r.skip(&src, "missing"),
            Ok(m) if m.is_dir() => r.tree(&src, &dst)?,
            Ok(m) if tree::is_cloud(&m) => r.skip(&src, "cloud"),
            Ok(m) => r.file(&src, &dst, m.len())?,
        }
        files.push(json!({ "key": it.key, "path": it.dest, "source": shown(&it.source), "bytes": r.done - before }));
    }
    Ok(Copied { files, skipped: r.skipped })
}

/// Teraz w UTC jako RFC 3339 (bez zewnętrznych bibliotek).
pub fn now_utc() -> String {
    let s = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let (days, rem) = (s.div_euclid(86400), s.rem_euclid(86400));
    // Algorytm „civil from days” (H. Hinnant).
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rem / 3600, rem / 60 % 60, rem % 60)
}

/// shrimp.json: `collected` to obiekt z windows/user/locale/apps/storeApps (collect.rs).
pub fn description(version: &str, sha256: &str, collected: &Value, c: &Copied) -> Value {
    let mut v = json!({
        "version": 1,
        "created": now_utc(),
        "installer": concat!("Shrimp Install ", env!("CARGO_PKG_VERSION")),
        "image": {
            "version": if version.is_empty() { Value::Null } else { json!(version) },
            "sha256": if sha256.len() == 64 { json!(sha256) } else { Value::Null },
        },
    });
    if let Some(o) = collected.as_object() {
        for (k, val) in o {
            v[k] = val.clone();
        }
    }
    v["files"] = json!(c.files);
    v["split"] = json!([]);
    v["skipped"] = json!(c.skipped);
    v
}

pub fn write_description(root: &Path, v: &Value) -> R<()> {
    let s = serde_json::to_string_pretty(v).map_err(|_| fail(3, "copy", Some("descWrite")))?;
    fs::write(root.join("shrimp.json"), s).map_err(|_| fail(3, "copy", Some("descWrite")))
}

/// boot/grub2/shrimp-lang.cfg na partycji FAT32: GRUB czyta z niego język menu (brak = angielski).
pub fn write_grub_lang(boot: &Path, tag: &str) {
    let dir = boot.join("boot").join("grub2");
    let _ = fs::create_dir_all(&dir);
    if fs::write(dir.join("shrimp-lang.cfg"), format!("set shrimp_lang={tag}\n")).is_err() {
        crate::log!("nie zapisano shrimp-lang.cfg (GRUB po angielsku)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plan_names() {
        let folders = vec![("documents".to_string(), PathBuf::from("/home/jan/Dokumenty"))];
        let p = plan(&folders, &["documents", "music"], &[r"C:\a\Zdjęcia".into(), r"D:\Zdjęcia".into(), r"E:\".into()]);
        let d: Vec<_> = p.iter().map(|i| i.dest.as_str()).collect();
        assert_eq!(d, ["files/documents", "files/other/Zdjęcia", "files/other/Zdjęcia (2)", "files/other/dysk"]);
    }

    #[test]
    fn copies_and_describes() {
        let t = std::env::temp_dir().join(format!("shrimp-transfer-test-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
        let src = t.join("Dokumenty");
        fs::create_dir_all(src.join("Praca")).unwrap();
        fs::write(src.join("Praca/raport.txt"), "x".repeat(1000)).unwrap();
        fs::write(src.join("desktop.ini"), "[.ShellClassInfo]").unwrap();
        let items = plan(&[("documents".into(), src.clone())], &["documents"], &[t.join("brak").to_string_lossy().into_owned()]);
        let cancel = AtomicBool::new(false);
        let mut n = 0;
        let c = copy(&t.join("usb").join(DIR), &items, &cancel, &mut |k| n += k).unwrap();
        assert_eq!(n, 1000);
        assert!(t.join("usb/shrimp-transfer/files/documents/Praca/raport.txt").exists());
        assert!(!t.join("usb/shrimp-transfer/files/documents/desktop.ini").exists());
        assert_eq!(c.files[0]["bytes"], 1000);
        assert_eq!(c.skipped[0]["reason"], "missing");
        let v = description("Beta 5", "", &json!({ "user": { "name": "jan" } }), &c);
        assert_eq!(v["user"]["name"], "jan");
        assert_eq!(v["image"]["sha256"], Value::Null);
        assert_eq!(v["split"], json!([]));
        let _ = fs::remove_dir_all(t);
    }

    #[test]
    fn date() {
        let d = now_utc();
        assert_eq!(d.len(), 20);
        assert!(d.starts_with("20"));
    }
}
