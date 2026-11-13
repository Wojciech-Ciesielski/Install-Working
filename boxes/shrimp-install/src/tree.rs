//! Drzewa plików: rozmiar bez dowiązań i plików tylko w chmurze, kopiowanie z postępem, błędy nośnika.
use crate::ctx::{fail, Fail};
use std::fs::{self, File, Metadata};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const BUF: usize = 4 * 1024 * 1024;

#[cfg(windows)]
const REPARSE: u32 = 0x400;
#[cfg(windows)]
const CLOUD_ONLY: u32 = 0x1000 /* OFFLINE */ | 0x0040_0000 /* RECALL_ON_DATA_ACCESS */ | 0x0004_0000 /* RECALL_ON_OPEN */;

/// Dowiązanie (np. stare „Moja muzyka” w Dokumentach) albo punkt ponownej analizy: nie jest plikiem użytkownika.
pub fn is_link(m: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & REPARSE != 0 && m.file_attributes() & CLOUD_ONLY == 0 || m.file_type().is_symlink()
    }
    #[cfg(not(windows))]
    {
        m.file_type().is_symlink()
    }
}

/// Plik tylko w chmurze (OneDrive „na żądanie”): odczyt pobrałby go z internetu.
pub fn is_cloud(m: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        m.file_attributes() & CLOUD_ONLY != 0
    }
    #[cfg(not(windows))]
    {
        let _ = m;
        false
    }
}

pub fn size(path: &Path, cancel: Option<&AtomicBool>) -> u64 {
    let Ok(m) = fs::symlink_metadata(path) else { return 0 };
    if is_cloud(&m) || is_link(&m) {
        return 0;
    }
    if !m.is_dir() {
        return m.len();
    }
    let Ok(rd) = fs::read_dir(path) else { return 0 };
    let mut total = 0;
    for e in rd.flatten() {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            break;
        }
        total += size(&e.path(), cancel);
    }
    total
}

/// Ścieżka Windows bez limitu 260 znaków (\\?\C:\…); inne systemy bez zmian.
pub fn long(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        let s = p.to_string_lossy();
        if !s.starts_with(r"\\") && p.is_absolute() {
            return PathBuf::from(format!(r"\\?\{s}"));
        }
    }
    p.to_path_buf()
}

/// Błąd zapisu na nośnik, który przerywa całość (pełny, odłączony); inny błąd dotyczy tylko jednego pliku.
pub fn media_error(phase: u8, e: &io::Error) -> Option<Fail> {
    let code = e.raw_os_error().unwrap_or(0);
    #[cfg(windows)]
    let (full, gone): (&[i32], &[i32]) = (&[112, 39], &[21, 1167, 55, 3, 29, 31, 1117]);
    #[cfg(not(windows))]
    let (full, gone): (&[i32], &[i32]) = (&[28], &[5, 6, 19]);
    if full.contains(&code) || e.kind() == io::ErrorKind::StorageFull {
        Some(fail(phase, "copy", Some("mediaFull")))
    } else if gone.contains(&code) {
        Some(fail(phase, "device", None))
    } else {
        None
    }
}

/// Plik zablokowany przez inny program (Windows: naruszenie udostępniania lub blokady).
pub fn is_locked(e: &io::Error) -> bool {
    matches!(e.raw_os_error(), Some(32) | Some(33))
}

pub enum CopyErr {
    Read(io::Error),
    Write(io::Error),
    Cancel,
}

/// Kopia jednego pliku z postępem i anulowaniem; czas modyfikacji zostaje.
pub fn copy_file(src: &Path, dst: &Path, buf: &mut [u8], cancel: &AtomicBool, on: &mut dyn FnMut(u64)) -> Result<u64, CopyErr> {
    let mut i = File::open(src).map_err(CopyErr::Read)?;
    let mtime = i.metadata().and_then(|m| m.modified()).ok();
    let mut o = File::create(dst).map_err(CopyErr::Write)?;
    let mut n = 0u64;
    loop {
        if cancel.load(Ordering::Relaxed) {
            drop(o);
            let _ = fs::remove_file(dst);
            return Err(CopyErr::Cancel);
        }
        let k = i.read(buf).map_err(CopyErr::Read)?;
        if k == 0 {
            break;
        }
        o.write_all(&buf[..k]).map_err(CopyErr::Write)?;
        n += k as u64;
        on(k as u64);
    }
    o.flush().map_err(CopyErr::Write)?;
    if let Some(t) = mtime {
        let _ = o.set_modified(t);
    }
    Ok(n)
}

/// Nazwy plików nośnika, których szuka system live (LiveOS/squashfs.img, LiveOS/shrimp-repo.squashfs).
/// Obraz bez drzewa Joliet Windows pokazuje z nazwami ISO 9660: wielkie litery i „_” zamiast „-”
/// (Beta 5: LIVEOS\SHRIMP_REPO.SQUASHFS); taki plik zapisujemy pod nazwą, której szuka Linux.
pub fn canonical_rel(rel: &Path) -> PathBuf {
    const KNOWN: [&str; 2] = ["squashfs.img", "shrimp-repo.squashfs"];
    let mut parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    if parts.first().is_some_and(|p| p.eq_ignore_ascii_case("LiveOS")) {
        parts[0] = "LiveOS".into();
        if parts.len() == 2 {
            let norm = parts[1].to_ascii_lowercase().replace('_', "-");
            if let Some(k) = KNOWN.iter().find(|k| **k == norm) {
                parts[1] = (*k).to_string();
            }
        }
    }
    parts.iter().collect()
}


#[cfg(test)]
mod canonical_tests {
    #[test]
    fn live_names_from_iso9660() {
        use std::path::{Path, PathBuf};
        assert_eq!(super::canonical_rel(Path::new("LIVEOS/SHRIMP_REPO.SQUASHFS")), PathBuf::from("LiveOS/shrimp-repo.squashfs"));
        assert_eq!(super::canonical_rel(Path::new("LiveOS/SQUASHFS.IMG")), PathBuf::from("LiveOS/squashfs.img"));
        assert_eq!(super::canonical_rel(Path::new("LiveOS/OTHER_FILE")), PathBuf::from("LiveOS/OTHER_FILE"));
        assert_eq!(super::canonical_rel(Path::new("EFI/BOOT/bootx64.efi")), PathBuf::from("EFI/BOOT/bootx64.efi"));
    }
}
