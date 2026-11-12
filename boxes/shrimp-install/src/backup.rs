//! Zawartość nośnika nie ginie przy jego przygotowaniu.
//!
//! 1. Przed formatowaniem ([`stage`]): wszystkie pliki z nośnika trafiają do
//!    %LOCALAPPDATA%\Shrimp\usb-backup\<id nośnika>\files, z SHA-256 liczonym przy odczycie.
//!    Kopia jest potem czytana z dysku jeszcze raz (bez pamięci podręcznej) i porównywana.
//!    Dopiero zgodna kopia pozwala sformatować nośnik. Każdy błąd kończy przygotowanie,
//!    zanim cokolwiek zostanie skasowane.
//! 2. Po zapisie Shrimp ([`write_back`]): pliki idą do `shrimp-backup/` na partycji exFAT gotowego nośnika,
//!    znowu z weryfikacją odczytem. manifest.tsv jest zapisywany na końcu: bez niego Setup nośnika nie rusza.
//!    Kopia w Windows znika dopiero po udanej weryfikacji na nośniku.
//! 3. Setup (CoreOS-Working: shrimp-usb-backup) po instalacji przenosi kopię na dysk komputera, a w onboardingu
//!    formatuje nośnik i przywraca na nim pliki.
//!
//! manifest.tsv (UTF-8, ścieżki z „/”, bez tabulatorów w nazwach, bo Windows ich nie dopuszcza):
//! ```text
//! #shrimp-backup 1
//! label<TAB>etykieta          fs<TAB>FAT32|exFAT|NTFS|      scheme<TAB>mbr|gpt
//! d<TAB>ścieżka               (folder, także pusty)
//! f<TAB>sha256<TAB>bajty<TAB>mtime_unix<TAB>części<TAB>ścieżka
//! ```
//! części = 0: jeden plik; N > 0: <ścieżka>.shrimp-part000 … (stare nośniki FAT32; na exFAT nie dzielimy).
use crate::ctx::{cancelled, fail, R};
use crate::manifest::hex;
use sha2::{Digest, Sha256};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, UNIX_EPOCH};

pub const LIVE_LABEL: &str = "SHRIMP_LIVE";
pub const BOOT_LABEL: &str = "SHRIMP_BOOT";
const MARGIN: u64 = 512 * 1024 * 1024;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Entry {
    pub rel: String,
    pub size: u64,
    pub mtime: i64,
    pub sha: [u8; 32],
    pub parts: u32,
    pub dir: bool,
}

#[derive(Debug, Clone, Default)]
pub struct List {
    pub e: Vec<Entry>,
    pub bytes: u64,
    pub label: String,
    pub fs: String,
    pub scheme: String,
}

/// Wolumin nośnika przed formatowaniem.
#[derive(Debug, Clone)]
pub struct Volume {
    pub root: PathBuf,
    pub label: String,
    pub fs: String,
}

struct Source {
    root: PathBuf,
    prefix: String,
    from_backup: bool,
}

// ---------- bufor (odczyt bez pamięci podręcznej wymaga w Windows bufora wyrównanego do sektora) ----------
pub struct Buf(Vec<u8>, usize);
impl Buf {
    pub fn new() -> Self {
        let v = vec![0u8; crate::tree::BUF + 4096];
        let off = (4096 - (v.as_ptr() as usize % 4096)) % 4096;
        Buf(v, off)
    }
    pub fn get(&mut self) -> &mut [u8] {
        let o = self.1;
        &mut self.0[o..o + crate::tree::BUF]
    }
}

fn rel_path(base: &Path, rel: &str) -> PathBuf {
    rel.split('/').fold(base.to_path_buf(), |p, c| p.join(c))
}

fn part_name(p: &Path, i: u32) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(format!(".shrimp-part{i:03}"));
    PathBuf::from(s)
}

fn open_read(p: &Path, nocache: bool) -> io::Result<File> {
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    if nocache {
        use std::os::windows::fs::OpenOptionsExt;
        o.custom_flags(0x2000_0000 /* FILE_FLAG_NO_BUFFERING */);
    }
    #[cfg(not(windows))]
    let _ = nocache;
    o.open(p)
}

/// Czyta plik (albo jego części) i liczy SHA-256; opcjonalnie zapisuje to samo do `out`.
fn hash_read(path: &Path, parts: u32, nocache: bool, mut out: Option<&mut File>, buf: &mut Buf, cancel: &AtomicBool, on: &mut dyn FnMut(u64)) -> io::Result<(u64, [u8; 32])> {
    let mut h = Sha256::new();
    let mut n = 0u64;
    let files: Vec<PathBuf> = if parts == 0 { vec![path.into()] } else { (0..parts).map(|i| part_name(path, i)).collect() };
    for f in files {
        let mut i = open_read(&f, nocache)?;
        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancel"));
            }
            let b = buf.get();
            let k = i.read(b)?;
            if k == 0 {
                break;
            }
            h.update(&b[..k]);
            if let Some(o) = out.as_deref_mut() {
                o.write_all(&b[..k])?;
            }
            n += k as u64;
            on(k as u64);
        }
    }
    Ok((n, h.finalize().into()))
}

// ---------- manifest ----------
pub fn write_manifest(path: &Path, l: &List) -> io::Result<()> {
    let mut s = String::from("#shrimp-backup 1\n");
    s += &format!("label\t{}\nfs\t{}\nscheme\t{}\n", l.label, l.fs, if l.scheme.is_empty() { "mbr" } else { &l.scheme });
    for e in &l.e {
        if e.dir {
            s += &format!("d\t{}\n", e.rel);
        } else {
            s += &format!("f\t{}\t{}\t{}\t{}\t{}\n", hex(&e.sha), e.size, e.mtime, e.parts, e.rel);
        }
    }
    let tmp = path.with_extension("tsv.tmp");
    {
        let mut f = File::create(&tmp)?;
        f.write_all(s.as_bytes())?;
        f.sync_all()?;
    }
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

fn unhex(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut d = [0u8; 32];
    for (i, b) in d.iter_mut().enumerate() {
        *b = u8::from_str_radix(s.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(d)
}

pub fn read_manifest(path: &Path) -> io::Result<List> {
    let bad = || io::Error::new(io::ErrorKind::InvalidData, "manifest.tsv");
    let s = fs::read_to_string(path)?;
    let mut l = List::default();
    let mut lines = s.lines();
    if lines.next() != Some("#shrimp-backup 1") {
        return Err(bad());
    }
    for line in lines {
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // Ścieżka (ostatnie pole) może zawierać wszystko poza tabulatorem.
        let f: Vec<&str> = line.splitn(6, '\t').collect();
        match (f[0], f.len()) {
            ("label", 2) => l.label = f[1].into(),
            ("fs", 2) => l.fs = f[1].into(),
            ("scheme", 2) => l.scheme = f[1].into(),
            ("d", 2) => l.e.push(Entry { rel: f[1].into(), dir: true, ..Default::default() }),
            ("f", 6) => {
                let e = Entry {
                    rel: f[5].into(),
                    sha: unhex(f[1]).ok_or_else(bad)?,
                    size: f[2].parse().map_err(|_| bad())?,
                    mtime: f[3].parse().map_err(|_| bad())?,
                    parts: f[4].parse().map_err(|_| bad())?,
                    dir: false,
                };
                l.bytes += e.size;
                l.e.push(e);
            }
            _ => return Err(bad()),
        }
    }
    Ok(l)
}

// ---------- 1. kopia przed formatowaniem ----------
fn skip_name(n: &str) -> bool {
    ["System Volume Information", "$RECYCLE.BIN", "RECYCLER"].iter().any(|x| x.eq_ignore_ascii_case(n))
}

/// Zbiera listę plików i folderów z drzewa nośnika (bez kopiowania).
fn scan(root: &Path, sub: &str, prefix: &str, l: &mut List, cancel: &AtomicBool) -> io::Result<()> {
    let dir = if sub.is_empty() { root.to_path_buf() } else { rel_path(root, sub) };
    let mut items: Vec<_> = fs::read_dir(&dir)?.collect::<io::Result<_>>()?;
    items.sort_by_key(|e| e.file_name());
    for e in items {
        if cancel.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancel"));
        }
        let name = e.file_name().to_string_lossy().into_owned();
        if sub.is_empty() && skip_name(&name) {
            continue;
        }
        let m = e.metadata()?;
        if crate::tree::is_link(&m) {
            continue;
        }
        let s = if sub.is_empty() { name } else { format!("{sub}/{name}") };
        let rel = format!("{prefix}{s}");
        if m.is_dir() {
            l.e.push(Entry { rel, dir: true, ..Default::default() });
            scan(root, &s, prefix, l, cancel)?;
        } else {
            let mtime = m.modified().ok().and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs() as i64);
            l.bytes += m.len();
            l.e.push(Entry { rel, size: m.len(), mtime, ..Default::default() });
        }
    }
    Ok(())
}

fn source_of<'a>(src: &'a [Source], rel: &'a str) -> Option<(&'a Source, &'a str)> {
    src.iter().rev().find(|s| rel.starts_with(&s.prefix)).map(|s| (s, &rel[s.prefix.len()..]))
}

fn is_cancel(e: &io::Error) -> bool {
    e.kind() == io::ErrorKind::Interrupted
}

/// Kopiuje listę ze źródeł do kopii w Windows i sprawdza ją drugim odczytem.
fn copy_to_staging(staging: &Path, src: &[Source], l: &mut List, check_known: bool, cancel: &AtomicBool, on: &mut dyn FnMut(u64)) -> R<()> {
    let files = staging.join("files");
    let read_err = |e: &io::Error| if is_cancel(e) { cancelled(2) } else { fail(2, "copy", Some("backupRead")) };
    fs::create_dir_all(&files).map_err(|_| fail(2, "space", Some("backupSpace")))?;
    let mut buf = Buf::new();
    for e in l.e.iter_mut() {
        let (s, inner) = source_of(src, &e.rel).ok_or_else(|| fail(2, "copy", Some("backupRead")))?;
        let dst = rel_path(&files, &e.rel);
        if e.dir {
            fs::create_dir_all(&dst).map_err(|_| fail(2, "space", Some("backupSpace")))?;
            continue;
        }
        if let Some(p) = dst.parent() {
            fs::create_dir_all(p).map_err(|_| fail(2, "space", Some("backupSpace")))?;
        }
        let mut out = File::create(&dst).map_err(|_| fail(2, "space", Some("backupSpace")))?;
        let from = rel_path(&s.root, inner);
        let (got, d) = hash_read(&from, if s.from_backup { e.parts } else { 0 }, false, Some(&mut out), &mut buf, cancel, on).map_err(|e| read_err(&e))?;
        out.sync_all().map_err(|_| fail(2, "space", Some("backupSpace")))?;
        let _ = out.set_modified(UNIX_EPOCH + Duration::from_secs(e.mtime.max(0) as u64));
        // Nośnik Shrimp: suma z jego manifestu musi się zgadzać, zanim zaufamy tej kopii.
        if got != e.size || (check_known && s.from_backup && d != e.sha) {
            return Err(fail(2, "copy", Some("backupRead")));
        }
        e.sha = d;
        e.parts = 0;
    }
    // Druga runda: kopia w Windows czytana z dysku (bez pamięci podręcznej) i porównana z sumami ze źródła.
    for e in l.e.iter().filter(|e| !e.dir) {
        let (got, d) = hash_read(&rel_path(&files, &e.rel), 0, true, None, &mut buf, cancel, on).map_err(|e| read_err(&e))?;
        if got != e.size || d != e.sha {
            return Err(fail(2, "copy", Some("backupRead")));
        }
    }
    Ok(())
}

/// Kopia zawartości nośnika do `staging` przed formatowaniem.
/// `media_size`: rozmiar nośnika, `shrimp_bytes`: ile zajmie Shrimp z plikami z Windows (sprawdzenie miejsca).
/// `on`: bajty odczytane i zapisane (postęp, łącznie 2 × bajty kopii).
#[allow(clippy::too_many_arguments)]
pub fn stage(staging: &Path, vols: &[Volume], scheme: &str, media_size: u64, shrimp_bytes: u64, free_space: impl Fn(&Path) -> Option<u64>, cancel: &AtomicBool, on: &mut dyn FnMut(u64, u64)) -> R<List> {
    // Poprzednia próba sformatowała już ten nośnik: oryginały są tylko w kopii, nie robimy jej od nowa.
    if staging.join("formatted").exists() {
        crate::log!("kopia nośnika: używam kopii z poprzedniej próby");
        return read_manifest(&staging.join("manifest.tsv")).map_err(|_| fail(2, "copy", Some("backupRead")));
    }
    let _ = fs::remove_dir_all(staging);
    fs::create_dir_all(staging).map_err(|_| fail(2, "space", Some("backupSpace")))?;
    let mut cur = List { scheme: scheme.into(), ..Default::default() };
    let mut src = Vec::new();
    let data: Vec<&Volume> = vols.iter().filter(|v| !v.label.eq_ignore_ascii_case(BOOT_LABEL)).collect();
    for (i, v) in data.iter().enumerate() {
        let prefix = if data.len() > 1 { format!("{}/", i + 1) } else { String::new() };
        if v.label.eq_ignore_ascii_case(LIVE_LABEL) {
            // Nośnik przygotowany już wcześniej: oryginalna zawartość jest w jego shrimp-backup.
            let b = v.root.join("shrimp-backup");
            if let Ok(old) = read_manifest(&b.join("manifest.tsv")) {
                if src.is_empty() {
                    (cur.label, cur.fs, cur.scheme) = (old.label.clone(), old.fs.clone(), old.scheme.clone());
                }
                for e in old.e {
                    cur.bytes += e.size;
                    cur.e.push(Entry { rel: format!("{prefix}{}", e.rel), ..e });
                }
                src.push(Source { root: b.join("files"), prefix, from_backup: true });
            }
            // Bez opisu: na nośniku jest tylko poprzedni Shrimp (obraz i pliki z Windows, które powstaną znowu).
            continue;
        }
        if src.is_empty() {
            (cur.label, cur.fs) = (v.label.clone(), v.fs.clone());
        }
        scan(&v.root, "", &prefix, &mut cur, cancel).map_err(|e| if is_cancel(&e) { cancelled(2) } else { fail(2, "copy", Some("backupRead")) })?;
        src.push(Source { root: v.root.clone(), prefix, from_backup: false });
    }
    // Miejsce: kopia w Windows i potem na nośniku obok Shrimp.
    if cur.bytes > 0 {
        if free_space(staging).is_some_and(|f| f < cur.bytes + MARGIN) {
            return Err(fail(2, "space", Some("backupSpace")));
        }
        if media_size != 0 && shrimp_bytes + cur.bytes + cur.bytes / 50 + cur.e.len() as u64 * 32768 + MARGIN > media_size {
            return Err(fail(2, "write", Some("backupMedia")));
        }
    }
    let total = cur.bytes * 2;
    let mut done = 0;
    copy_to_staging(staging, &src, &mut cur, true, cancel, &mut |n| {
        done += n;
        on(done, total)
    })?;
    write_manifest(&staging.join("manifest.tsv"), &cur).map_err(|_| fail(2, "space", Some("backupSpace")))?;
    Ok(cur)
}

/// Tuż przed formatowaniem: od teraz oryginały są tylko w kopii.
pub fn mark_formatted(staging: &Path) {
    if let Ok(f) = File::create(staging.join("formatted")) {
        let _ = f.sync_all();
    }
}

// ---------- 2. kopia na gotowy nośnik ----------
/// Zapis kopii do `<live>/shrimp-backup`, odczyt i porównanie sum, manifest, potem usunięcie kopii w Windows.
pub fn write_back(staging: &Path, l: &List, live: &Path, cancel: &AtomicBool, on: &mut dyn FnMut(u64)) -> R<()> {
    let r = (|| -> io::Result<()> {
        let bdir = live.join("shrimp-backup");
        let fdir = bdir.join("files");
        fs::create_dir_all(&fdir)?;
        hide(&bdir);
        let sfiles = staging.join("files");
        let mut buf = Buf::new();
        for e in &l.e {
            let dst = rel_path(&fdir, &e.rel);
            if e.dir {
                fs::create_dir_all(&dst)?;
                continue;
            }
            if let Some(p) = dst.parent() {
                fs::create_dir_all(p)?;
            }
            let mut out = File::create(&dst)?;
            hash_read(&rel_path(&sfiles, &e.rel), 0, false, Some(&mut out), &mut buf, cancel, on)?;
            out.sync_all()?;
            let _ = out.set_modified(UNIX_EPOCH + Duration::from_secs(e.mtime.max(0) as u64));
        }
        for e in l.e.iter().filter(|e| !e.dir) {
            let (got, d) = hash_read(&rel_path(&fdir, &e.rel), 0, true, None, &mut buf, cancel, on)?;
            if got != e.size || d != e.sha {
                return Err(io::Error::new(io::ErrorKind::InvalidData, "suma"));
            }
        }
        let mut out = l.clone();
        out.e.iter_mut().for_each(|e| e.parts = 0);
        let man = bdir.join("manifest.tsv");
        write_manifest(&man, &out)?;
        if read_manifest(&man)?.e.len() != l.e.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "manifest"));
        }
        Ok(())
    })();
    match r {
        Ok(()) => {
            crate::log!("zawartość nośnika ({} pozycji, {} B) jest w shrimp-backup", l.e.len(), l.bytes);
            let _ = fs::remove_dir_all(staging);
            Ok(())
        }
        Err(e) if is_cancel(&e) => Err(cancelled(3)),
        Err(e) => {
            crate::log!("kopia zawartości nośnika została w {} ({e})", staging.display());
            Err(fail(3, "copy", Some("backupVerify")))
        }
    }
}

fn hide(p: &Path) {
    #[cfg(windows)]
    {
        let _ = std::process::Command::new("attrib").arg("+h").arg(p).status();
    }
    #[cfg(not(windows))]
    let _ = p;
}

/// Test bez nośnika (CI, `--backup-test ŹRÓDŁO CEL KOPIA`): kopia, zapis, potem ponowne przygotowanie
/// tego samego „nośnika” z jego shrimp-backup, z weryfikacją sum.
pub fn self_test(src: &Path, dst: &Path, work: &Path) -> Result<String, String> {
    let cancel = AtomicBool::new(false);
    let staging = work.join("stage");
    let vol = |root: &Path, label: &str| Volume { root: root.into(), label: label.into(), fs: "exFAT".into() };
    let l = stage(&staging, &[vol(src, "TEST")], "mbr", 0, 0, |_| None, &cancel, &mut |_, _| {}).map_err(|f| format!("kopia do Windows nie powiodła się: {f:?}"))?;
    fs::create_dir_all(dst).map_err(|e| e.to_string())?;
    write_back(&staging, &l, dst, &cancel, &mut |_| {}).map_err(|f| format!("zapis na nośnik nie powiódł się: {f:?}"))?;
    if staging.exists() {
        return Err("kopia w Windows nie została usunięta".into());
    }
    let again = stage(&staging, &[vol(dst, LIVE_LABEL)], "gpt", 0, 0, |_| None, &cancel, &mut |_, _| {}).map_err(|f| format!("ponowna kopia z shrimp-backup nie powiodła się: {f:?}"))?;
    if again.e.len() != l.e.len() || again.bytes != l.bytes || again.label != "TEST" {
        return Err("manifest na nośniku jest niepoprawny".into());
    }
    Ok(format!("kopia nośnika: {} pozycji, {} B, zgodna", again.e.len(), again.bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("shrimp-backup-test-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    #[test]
    fn round_trip() {
        let t = tmp("rt");
        let src = t.join("usb");
        fs::create_dir_all(src.join("DCIM/100")).unwrap();
        fs::create_dir_all(src.join("Pusty folder")).unwrap();
        fs::create_dir_all(src.join("System Volume Information")).unwrap();
        fs::write(src.join("notatka.txt"), "pendrive użytkownika").unwrap();
        fs::write(src.join("DCIM/100/a.bin"), vec![7u8; 5_000_000]).unwrap();
        fs::write(src.join("Zażółć.txt"), "").unwrap();
        let msg = self_test(&src, &t.join("medium"), &t.join("work")).unwrap();
        assert!(msg.contains("6 pozycji"), "{msg}");
        let m = read_manifest(&t.join("medium/shrimp-backup/manifest.tsv")).unwrap();
        assert!(m.e.iter().any(|e| e.rel == "Pusty folder" && e.dir));
        assert!(!m.e.iter().any(|e| e.rel.starts_with("System Volume")));
        assert_eq!(fs::read(t.join("medium/shrimp-backup/files/DCIM/100/a.bin")).unwrap().len(), 5_000_000);
        let _ = fs::remove_dir_all(t);
    }

    #[test]
    fn old_split_backup_is_joined() {
        // Stary nośnik FAT32: plik w częściach w shrimp-backup.
        let t = tmp("split");
        let live = t.join("live");
        let files = live.join("shrimp-backup/files");
        fs::create_dir_all(&files).unwrap();
        fs::write(files.join("big.iso.shrimp-part000"), "abc").unwrap();
        fs::write(files.join("big.iso.shrimp-part001"), "de").unwrap();
        let l = List {
            e: vec![Entry { rel: "big.iso".into(), size: 5, sha: Sha256::digest(b"abcde").into(), parts: 2, ..Default::default() }],
            bytes: 5,
            label: "USB".into(),
            fs: "FAT32".into(),
            scheme: "mbr".into(),
        };
        write_manifest(&live.join("shrimp-backup/manifest.tsv"), &l).unwrap();
        let cancel = AtomicBool::new(false);
        let boot = Volume { root: t.join("boot"), label: BOOT_LABEL.into(), fs: "FAT32".into() };
        let got = stage(&t.join("stage"), &[boot, Volume { root: live, label: LIVE_LABEL.into(), fs: "exFAT".into() }], "gpt", 0, 0, |_| None, &cancel, &mut |_, _| {}).unwrap();
        assert_eq!(got.e[0].parts, 0);
        assert_eq!(fs::read(t.join("stage/files/big.iso")).unwrap(), b"abcde");
        let _ = fs::remove_dir_all(t);
    }

    #[test]
    fn bad_sum_stops_before_format() {
        let t = tmp("bad");
        let live = t.join("live");
        fs::create_dir_all(live.join("shrimp-backup/files")).unwrap();
        fs::write(live.join("shrimp-backup/files/a.txt"), "zmienione").unwrap();
        let l = List { e: vec![Entry { rel: "a.txt".into(), size: 9, sha: [1; 32], ..Default::default() }], bytes: 9, ..Default::default() };
        write_manifest(&live.join("shrimp-backup/manifest.tsv"), &l).unwrap();
        let cancel = AtomicBool::new(false);
        let r = stage(&t.join("stage"), &[Volume { root: live, label: LIVE_LABEL.into(), fs: "exFAT".into() }], "gpt", 0, 0, |_| None, &cancel, &mut |_, _| {});
        assert_eq!(r.unwrap_err().message.as_deref(), Some("backupRead"));
        let _ = fs::remove_dir_all(t);
    }
}
