//! Zakładki Systemy i Odinstaluj Shrimp: wpisy UEFI (BootOrder, BootNext, Boot####) przez zmienne
//! firmware Windows i pliki Shrimp na partycji EFI (montowanej na chwilę przez mountvol /S).
use super::boot;
use crate::efi::{self, Kind};
use serde_json::{json, Value};
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;
use windows::core::HSTRING;
use windows::Win32::System::WindowsProgramming::{GetFirmwareEnvironmentVariableW, SetFirmwareEnvironmentVariableW};

const EFI_GLOBAL: &str = "{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}";
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

fn get(name: &str) -> Option<Vec<u8>> {
    let mut buf = vec![0u8; 8192];
    let n = unsafe { GetFirmwareEnvironmentVariableW(&HSTRING::from(name), &HSTRING::from(EFI_GLOBAL), Some(buf.as_mut_ptr() as _), buf.len() as u32) } as usize;
    (n > 0).then(|| {
        buf.truncate(n);
        buf
    })
}

/// Zapis zmiennej; puste dane usuwają ją.
fn set(name: &str, data: &[u8]) -> Result<(), String> {
    let ptr = if data.is_empty() { None } else { Some(data.as_ptr() as _) };
    unsafe { SetFirmwareEnvironmentVariableW(&HSTRING::from(name), &HSTRING::from(EFI_GLOBAL), ptr, data.len() as u32) }
        .map_err(|e| format!("{name}: {e}"))
}

struct Entry {
    num: u16,
    kind: Kind,
    description: String,
}

fn ready() -> Result<(), String> {
    if boot::firmware() != "uefi" {
        return Err("noUefi".into());
    }
    if !boot::privilege("SeSystemEnvironmentPrivilege") {
        return Err("noPriv".into());
    }
    Ok(())
}

/// Wpisy z BootOrder i (dla wpisów spoza kolejności) Boot0000–Boot00FF.
fn entries() -> (Vec<u16>, Vec<Entry>) {
    let order = get("BootOrder").map(|b| efi::parse_order(&b)).unwrap_or_default();
    let mut nums: Vec<u16> = order.clone();
    nums.extend((0..0x100u16).filter(|n| !order.contains(n)));
    let list = nums
        .into_iter()
        .filter_map(|num| {
            let o = efi::parse_load_option(&get(&format!("Boot{num:04X}"))?)?;
            Some(Entry { num, kind: efi::kind(&o), description: o.description })
        })
        .collect();
    (order, list)
}

fn first_of(order: &[u16], list: &[Entry], kind: Kind) -> Option<u16> {
    order.iter().copied().find(|n| list.iter().any(|e| e.num == *n && e.kind == kind)).or_else(|| list.iter().find(|e| e.kind == kind).map(|e| e.num))
}

/// `{ firmware, shrimp: { id, name } | null, default: "windows" | "shrimp" | null }`
pub fn state() -> Value {
    let firmware = boot::firmware();
    if ready().is_err() {
        return json!({ "firmware": firmware, "shrimp": null, "default": null });
    }
    let (order, list) = entries();
    let shrimp = first_of(&order, &list, Kind::Shrimp);
    let def = order.iter().find_map(|n| list.iter().find(|e| e.num == *n && matches!(e.kind, Kind::Shrimp | Kind::Windows))).map(|e| match e.kind {
        Kind::Shrimp => "shrimp",
        _ => "windows",
    });
    json!({
        "firmware": firmware,
        "shrimp": shrimp.map(|n| json!({ "id": format!("{n:04X}"), "name": list.iter().find(|e| e.num == n).map(|e| e.description.clone()) })),
        "default": def,
    })
}

/// „Uruchom ponownie w Shrimp”: BootNext na wpis Shrimp i restart.
pub fn boot_shrimp() -> Result<(), String> {
    ready()?;
    let (order, list) = entries();
    let n = first_of(&order, &list, Kind::Shrimp).ok_or("noShrimp")?;
    set("BootNext", &n.to_le_bytes())?;
    crate::log!("BootNext = Boot{n:04X} (Shrimp)");
    boot::disable_fast_startup();
    if boot::reboot() { Ok(()) } else { Err("rebootFail".into()) }
}

/// Domyślny system: wpis Windows albo Shrimp na początek BootOrder.
pub fn set_default(which: &str) -> Result<(), String> {
    ready()?;
    let kind = match which {
        "shrimp" => Kind::Shrimp,
        "windows" => Kind::Windows,
        _ => return Err("unknown".into()),
    };
    let (order, list) = entries();
    let n = first_of(&order, &list, kind).ok_or(if kind == Kind::Shrimp { "noShrimp" } else { "noWindows" })?;
    let new = efi::reorder(&order, Some(n), &[]);
    set("BootOrder", &efi::order_bytes(&new))?;
    crate::log!("BootOrder = {new:04X?} (domyślnie {which})");
    Ok(())
}

/// Restart do ustawień UEFI (Windows 8+: shutdown /r /fw).
pub fn firmware_reboot() -> Result<(), String> {
    if boot::firmware() != "uefi" {
        return Err("noUefi".into());
    }
    boot::privilege("SeShutdownPrivilege");
    let st = Command::new("shutdown").args(["/r", "/fw", "/t", "0"]).creation_flags(CREATE_NO_WINDOW).status().map_err(|e| e.to_string())?;
    if st.success() { Ok(()) } else { Err("rebootFail".into()) }
}

/// Partycja EFI pod wolną literą na czas `f` (mountvol X: /S, potem /D).
fn with_esp<T>(f: impl FnOnce(&std::path::Path) -> Result<T, String>) -> Result<T, String> {
    let letter = ('R'..='Z').rev().find(|c| !PathBuf::from(format!("{c}:\\")).exists()).ok_or("noLetter")?;
    let drive = format!("{letter}:");
    let mount = |args: &[&str]| Command::new("mountvol").args(args).creation_flags(CREATE_NO_WINDOW).status().map(|s| s.success()).unwrap_or(false);
    if !mount(&[&drive, "/S"]) {
        return Err("espMount".into());
    }
    let r = f(&PathBuf::from(format!("{drive}\\")));
    if !mount(&[&drive, "/D"]) {
        crate::log!("mountvol {drive} /D nie powiódł się");
    }
    r
}

/// Odinstalowanie: wpisy Shrimp (i dawny wpis nośnika) znikają z UEFI, Windows na początku BootOrder,
/// pliki Shrimp z partycji EFI usunięte (efi::esp_plan). Partycje Shrimp zostają (Zarządzanie dyskami).
pub fn uninstall() -> Result<Value, String> {
    ready()?;
    let (order, list) = entries();
    let drop: Vec<u16> = list.iter().filter(|e| matches!(e.kind, Kind::Shrimp | Kind::Medium)).map(|e| e.num).collect();
    if !drop.iter().any(|n| list.iter().any(|e| e.num == *n && e.kind == Kind::Shrimp)) {
        return Err("noShrimp".into());
    }
    let windows = first_of(&order, &list, Kind::Windows);
    // Najpierw pliki: bez nich wpis i tak by nie wystartował, a przy błędzie montowania nic się nie zmienia.
    let files = with_esp(|esp| {
        let plan = efi::esp_plan(esp);
        efi::apply(&plan, esp)
    })?;
    let new = efi::reorder(&order, windows, &drop);
    set("BootOrder", &efi::order_bytes(&new))?;
    if get("BootNext").is_some_and(|b| efi::parse_order(&b).first().is_some_and(|n| drop.contains(n))) {
        let _ = set("BootNext", &[]);
    }
    for n in &drop {
        set(&format!("Boot{n:04X}"), &[])?;
    }
    crate::log!("odinstalowano Shrimp: wpisy {drop:04X?}, BootOrder {new:04X?}, pliki {files:?}");
    Ok(json!({ "removed": drop.iter().map(|n| format!("Boot{n:04X}")).chain(files).collect::<Vec<_>>() }))
}

/// Zarządzanie dyskami (partycje Shrimp usuwa użytkownik sam).
pub fn open_disks() {
    let _ = Command::new("mmc").arg("diskmgmt.msc").spawn();
}
