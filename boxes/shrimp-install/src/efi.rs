//! Wpisy rozruchowe UEFI i pliki Shrimp na partycji EFI: część bez Windows (testowana na Linuksie).
//! Zakładki Systemy i Odinstaluj Shrimp (src/win/systems.rs) czytają i zapisują zmienne przez Windows.
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Shrimp,
    Windows,
    /// Wpis „Shrimp Install” z jednorazowego startu z nośnika (src/win/boot.rs).
    Medium,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoadOption {
    pub active: bool,
    pub description: String,
    /// Ścieżka pliku z węzła File(…), np. „\EFI\systemd\systemd-bootx64.efi”.
    pub file: Option<String>,
}

fn utf16_until_nul(b: &[u8]) -> (String, usize) {
    let mut units = vec![];
    let mut i = 0;
    while i + 1 < b.len() {
        let u = u16::from_le_bytes([b[i], b[i + 1]]);
        i += 2;
        if u == 0 {
            break;
        }
        units.push(u);
    }
    (String::from_utf16_lossy(&units), i)
}

/// EFI_LOAD_OPTION: atrybuty (u32), długość listy ścieżek (u16), opis UTF-16 z zerem, ścieżki urządzeń.
pub fn parse_load_option(b: &[u8]) -> Option<LoadOption> {
    if b.len() < 8 {
        return None;
    }
    let attrs = u32::from_le_bytes(b[0..4].try_into().ok()?);
    let list_len = u16::from_le_bytes(b[4..6].try_into().ok()?) as usize;
    let (description, used) = utf16_until_nul(&b[6..]);
    let start = 6 + used;
    let list = b.get(start..(start + list_len).min(b.len()))?;
    let mut file = None;
    let mut i = 0;
    while i + 4 <= list.len() {
        let (t, st) = (list[i], list[i + 1]);
        let len = u16::from_le_bytes([list[i + 2], list[i + 3]]) as usize;
        if len < 4 || i + len > list.len() {
            break;
        }
        if t == 0x7F && st == 0xFF {
            break;
        }
        if t == 4 && st == 4 {
            file = Some(utf16_until_nul(&list[i + 4..i + len]).0);
        }
        i += len;
    }
    Some(LoadOption { active: attrs & 1 == 1, description, file })
}

pub fn kind(o: &LoadOption) -> Kind {
    let d = o.description.to_ascii_lowercase();
    let f = o.file.as_deref().unwrap_or("").to_ascii_lowercase().replace('/', "\\");
    if d == "shrimp install" {
        Kind::Medium
    } else if d.contains("windows") || f.contains("\\efi\\microsoft\\") {
        Kind::Windows
    } else if d.contains("shrimp") || d.contains("linux boot manager") || d.contains("opensuse")
        || f.contains("\\efi\\systemd\\") || f.contains("\\efi\\opensuse\\") || f.contains("\\efi\\shrimp\\")
    {
        Kind::Shrimp
    } else {
        Kind::Other
    }
}

/// BootOrder (u16 LE) ↔ lista numerów.
pub fn parse_order(b: &[u8]) -> Vec<u16> {
    b.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).collect()
}

pub fn order_bytes(v: &[u16]) -> Vec<u8> {
    v.iter().flat_map(|n| n.to_le_bytes()).collect()
}

/// Kolejność z `first` na początku, bez numerów z `drop`.
pub fn reorder(order: &[u16], first: Option<u16>, drop: &[u16]) -> Vec<u16> {
    let mut v: Vec<u16> = first.into_iter().collect();
    v.extend(order.iter().copied().filter(|n| Some(*n) != first && !drop.contains(n)));
    v
}

/// Co usunąć z partycji EFI przy odinstalowaniu Shrimp.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    RemoveDir(PathBuf),
    /// Zapasowy \EFI\BOOT\BOOTX64.EFI to systemd-boot Shrimp: na jego miejsce menedżer rozruchu Windows.
    Replace { target: PathBuf, from: PathBuf },
}

/// Katalogi, których plan nigdy nie rusza (Windows i firmware).
const KEEP: [&str; 4] = ["efi", "boot", "microsoft", "system volume information"];

/// Plan dla partycji EFI zamontowanej w `esp`: katalogi systemd-boot i Shrimp, katalogi jąder z wpisów
/// loader/entries (np. „opensuse-leap/6.12…”) i zapasowy program startowy, jeśli to systemd-boot.
pub fn esp_plan(esp: &Path) -> Vec<Step> {
    let mut dirs: Vec<PathBuf> = ["EFI/systemd", "EFI/opensuse", "EFI/shrimp", "loader"].iter().map(|d| esp.join(d)).collect();
    if let Ok(rd) = fs::read_dir(esp.join("loader/entries")) {
        for e in rd.flatten() {
            let Ok(txt) = fs::read_to_string(e.path()) else { continue };
            for line in txt.lines() {
                let mut it = line.split_whitespace();
                if !matches!(it.next(), Some("linux" | "initrd" | "efi" | "devicetree")) {
                    continue;
                }
                let Some(p) = it.next() else { continue };
                let top = p.trim_start_matches(['/', '\\']).split(['/', '\\']).next().unwrap_or("");
                if !top.is_empty() && top != "." && top != ".." && !KEEP.contains(&top.to_ascii_lowercase().as_str()) {
                    dirs.push(esp.join(top));
                }
            }
        }
    }
    dirs.sort();
    dirs.dedup();
    let mut plan: Vec<Step> = dirs.into_iter().filter(|d| d.is_dir()).map(Step::RemoveDir).collect();
    let fallback = esp.join("EFI/BOOT/BOOTX64.EFI");
    let windows = esp.join("EFI/Microsoft/Boot/bootmgfw.efi");
    let is_sd = fs::read(&fallback).is_ok_and(|b| b.windows(12).any(|w| w == b"systemd-boot"));
    if is_sd && windows.is_file() {
        plan.push(Step::Replace { target: fallback, from: windows });
    }
    plan
}

/// Wykonuje plan; zwraca opis tego, co zrobiono (do dziennika i odpowiedzi).
pub fn apply(plan: &[Step], esp: &Path) -> Result<Vec<String>, String> {
    let mut done = vec![];
    for s in plan {
        match s {
            Step::RemoveDir(d) => {
                fs::remove_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
                done.push(d.strip_prefix(esp).unwrap_or(d).display().to_string());
            }
            Step::Replace { target, from } => {
                fs::copy(from, target).map_err(|e| format!("{}: {e}", target.display()))?;
                done.push(format!("{} ← {}", target.strip_prefix(esp).unwrap_or(target).display(), from.strip_prefix(esp).unwrap_or(from).display()));
            }
        }
    }
    Ok(done)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn utf16z(s: &str) -> Vec<u8> {
        s.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect()
    }

    fn option(desc: &str, file: &str) -> Vec<u8> {
        let mut hd = vec![4u8, 1, 42, 0];
        hd.extend([0u8; 38]);
        let f = utf16z(file);
        let mut fp = vec![4u8, 4];
        fp.extend(((4 + f.len()) as u16).to_le_bytes());
        fp.extend(f);
        let end = [0x7Fu8, 0xFF, 4, 0];
        let mut o = 1u32.to_le_bytes().to_vec();
        o.extend(((hd.len() + fp.len() + end.len()) as u16).to_le_bytes());
        o.extend(utf16z(desc));
        o.extend(hd);
        o.extend(fp);
        o.extend(end);
        o
    }

    #[test]
    fn load_options() {
        let o = parse_load_option(&option("Linux Boot Manager", r"\EFI\systemd\systemd-bootx64.efi")).unwrap();
        assert!(o.active);
        assert_eq!(o.file.as_deref(), Some(r"\EFI\systemd\systemd-bootx64.efi"));
        assert_eq!(kind(&o), Kind::Shrimp);
        let w = parse_load_option(&option("Windows Boot Manager", r"\EFI\Microsoft\Boot\bootmgfw.efi")).unwrap();
        assert_eq!(kind(&w), Kind::Windows);
        let m = parse_load_option(&option("Shrimp Install", r"\EFI\BOOT\BOOTX64.EFI")).unwrap();
        assert_eq!(kind(&m), Kind::Medium);
        assert_eq!(parse_load_option(&[1, 2, 3]), None);
    }

    #[test]
    fn orders() {
        assert_eq!(parse_order(&order_bytes(&[2, 1, 0x10])), [2, 1, 0x10]);
        assert_eq!(reorder(&[2, 1, 3], Some(1), &[]), [1, 2, 3]);
        assert_eq!(reorder(&[2, 1, 3], Some(1), &[2]), [1, 3]);
        assert_eq!(reorder(&[2, 1, 3], None, &[3]), [2, 1]);
    }

    #[test]
    fn plan_keeps_windows() {
        let t = std::env::temp_dir().join(format!("shrimp-esp-{}", std::process::id()));
        let _ = fs::remove_dir_all(&t);
        for d in ["EFI/systemd", "EFI/Microsoft/Boot", "EFI/BOOT", "loader/entries", "opensuse-leap/6.12.0", "Dell"] {
            fs::create_dir_all(t.join(d)).unwrap();
        }
        fs::write(t.join("EFI/Microsoft/Boot/bootmgfw.efi"), b"MZ windows").unwrap();
        fs::write(t.join("EFI/BOOT/BOOTX64.EFI"), b"MZ ... #### LoaderInfo: systemd-boot 257 ####").unwrap();
        fs::write(
            t.join("loader/entries/shrimp-6.12.conf"),
            "title Shrimp\nlinux /opensuse-leap/6.12.0/linux-abc\ninitrd /opensuse-leap/6.12.0/initrd-abc\noptions root=UUID=x\n",
        )
        .unwrap();
        fs::write(t.join("loader/entries/bad.conf"), "linux /EFI/Microsoft/evil\nefi /../x\n").unwrap();
        let plan = esp_plan(&t);
        let removed: Vec<String> = plan
            .iter()
            .filter_map(|s| if let Step::RemoveDir(d) = s { Some(d.strip_prefix(&t).unwrap().display().to_string()) } else { None })
            .collect();
        assert_eq!(removed, ["EFI/systemd", "loader", "opensuse-leap"]);
        assert!(plan.contains(&Step::Replace { target: t.join("EFI/BOOT/BOOTX64.EFI"), from: t.join("EFI/Microsoft/Boot/bootmgfw.efi") }));
        let done = apply(&plan, &t).unwrap();
        assert_eq!(done.len(), 4);
        assert!(t.join("EFI/Microsoft/Boot/bootmgfw.efi").is_file());
        assert!(t.join("Dell").is_dir());
        assert_eq!(fs::read(t.join("EFI/BOOT/BOOTX64.EFI")).unwrap(), b"MZ windows");
        let _ = fs::remove_dir_all(&t);
    }
}
