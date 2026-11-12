//! Drobne operacje na plikach (bez zależności): kopiowanie z prawami i dowiązaniami, lista plików.

use std::fs;
use std::os::unix::fs::{symlink, PermissionsExt};
use std::path::{Path, PathBuf};

pub fn chmod(p: &Path, mode: u32) -> Result<(), String> {
    fs::set_permissions(p, fs::Permissions::from_mode(mode)).map_err(|e| format!("{}: {e}", p.display()))
}

/// Kopiuje plik albo dowiązanie (jako dowiązanie), tworzy katalogi nadrzędne, zachowuje prawa.
pub fn copy_file(src: &Path, dst: &Path) -> Result<(), String> {
    let err = |e: std::io::Error| format!("{} → {}: {e}", src.display(), dst.display());
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent).map_err(err)?;
    }
    let meta = fs::symlink_metadata(src).map_err(err)?;
    if dst.symlink_metadata().is_ok() {
        fs::remove_file(dst).map_err(err)?;
    }
    if meta.file_type().is_symlink() {
        symlink(fs::read_link(src).map_err(err)?, dst).map_err(err)?;
    } else {
        fs::copy(src, dst).map_err(err)?;
    }
    Ok(())
}

/// Wszystkie pliki i dowiązania pod `dir` (ścieżki względne, posortowane), bez wchodzenia w dowiązania.
pub fn files(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    walk(dir, Path::new(""), &[], &mut out)?;
    out.sort();
    Ok(out)
}

fn walk(base: &Path, rel: &Path, skip: &[&str], out: &mut Vec<PathBuf>) -> Result<(), String> {
    let dir = base.join(rel);
    let rd = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    for e in rd.flatten() {
        let name = e.file_name();
        if skip.iter().any(|s| name == *s) {
            continue;
        }
        let r = rel.join(&name);
        let ft = e.file_type().map_err(|e| e.to_string())?;
        if ft.is_dir() {
            walk(base, &r, skip, out)?;
        } else {
            out.push(r);
        }
    }
    Ok(())
}

/// Kopiuje drzewo katalogów, pomijając katalogi i pliki o nazwach z `skip` (np. target, node_modules).
pub fn copy_tree(src: &Path, dst: &Path, skip: &[&str]) -> Result<usize, String> {
    let mut list = Vec::new();
    walk(src, Path::new(""), skip, &mut list)?;
    fs::create_dir_all(dst).map_err(|e| format!("{}: {e}", dst.display()))?;
    for rel in &list {
        copy_file(&src.join(rel), &dst.join(rel))?;
    }
    Ok(list.len())
}
