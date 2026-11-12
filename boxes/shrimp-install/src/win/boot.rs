//! Jednorazowy start z nośnika, bez wchodzenia do ustawień UEFI.
//!
//! Tworzy wpis rozruchowy UEFI Boot#### „Shrimp Install” wskazujący \EFI\BOOT\BOOTX64.EFI na partycji FAT32
//! nośnika (krótka ścieżka HD(…)/File(…), którą firmware dopasowuje po identyfikatorze partycji) i ustawia
//! BootNext. Kolejność startu (BootOrder) się nie zmienia, więc kolejne uruchomienie wraca do zwykłego porządku.
use super::disk;
use windows::core::{HSTRING, PCWSTR};
use windows::Win32::Foundation::{CloseHandle, HANDLE, LUID};
use windows::Win32::Security::{AdjustTokenPrivileges, LookupPrivilegeValueW, SE_PRIVILEGE_ENABLED, TOKEN_ADJUST_PRIVILEGES, TOKEN_PRIVILEGES, TOKEN_QUERY};
use windows::Win32::System::Shutdown::{ExitWindowsEx, EWX_REBOOT, SHTDN_REASON_FLAG_PLANNED, SHTDN_REASON_MAJOR_OPERATINGSYSTEM, SHTDN_REASON_MINOR_INSTALLATION};
use windows::Win32::System::SystemInformation::{GetFirmwareType, FIRMWARE_TYPE, FirmwareTypeBios, FirmwareTypeUefi};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::Win32::System::WindowsProgramming::{GetFirmwareEnvironmentVariableW, SetFirmwareEnvironmentVariableW};

const EFI_GLOBAL: &str = "{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}";
const DESCRIPTION: &str = "Shrimp Install";
const LOADER: &str = r"\EFI\BOOT\BOOTX64.EFI";

fn privilege(name: &str) -> bool {
    unsafe {
        let mut tok = HANDLE::default();
        if OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &mut tok).is_err() {
            return false;
        }
        let mut luid = LUID::default();
        let mut ok = LookupPrivilegeValueW(PCWSTR::null(), &HSTRING::from(name), &mut luid).is_ok();
        if ok {
            let mut tp = TOKEN_PRIVILEGES { PrivilegeCount: 1, ..Default::default() };
            tp.Privileges[0].Luid = luid;
            tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
            ok = AdjustTokenPrivileges(tok, false, Some(&tp), 0, None, None).is_ok()
                && windows::Win32::Foundation::GetLastError().is_ok();
        }
        let _ = CloseHandle(tok);
        ok
    }
}

pub fn firmware() -> &'static str {
    let mut fw = FIRMWARE_TYPE::default();
    unsafe {
        let _ = GetFirmwareType(&mut fw);
    }
    if fw == FirmwareTypeUefi { "uefi" } else if fw == FirmwareTypeBios { "bios" } else { "unknown" }
}

/// „{12345678-9ABC-DEF0-1234-56789ABCDEF0}” → bajty GUID w kolejności EFI (pierwsze trzy pola little-endian).
pub fn guid_bytes(s: &str) -> Option<[u8; 16]> {
    let h: String = s.chars().filter(|c| c.is_ascii_hexdigit()).collect();
    if h.len() != 32 {
        return None;
    }
    let b: Vec<u8> = (0..16).map(|i| u8::from_str_radix(&h[i * 2..i * 2 + 2], 16).unwrap()).collect();
    let mut o = [0u8; 16];
    o[..4].copy_from_slice(&[b[3], b[2], b[1], b[0]]);
    o[4..6].copy_from_slice(&[b[5], b[4]]);
    o[6..8].copy_from_slice(&[b[7], b[6]]);
    o[8..].copy_from_slice(&b[8..]);
    Some(o)
}

fn utf16z(s: &str) -> Vec<u8> {
    s.encode_utf16().chain(Some(0)).flat_map(u16::to_le_bytes).collect()
}

/// EFI_LOAD_OPTION: atrybuty, długość listy ścieżek, opis, ścieżka HD(…)/File(…)/End.
pub fn load_option(p: &disk::BootPart) -> Option<Vec<u8>> {
    let sector = if p.sector == 0 { 512 } else { p.sector as u64 };
    let mut hd = vec![4u8, 1, 42, 0]; // Media / Hard Drive, długość 42
    hd.extend_from_slice(&p.number.to_le_bytes());
    hd.extend_from_slice(&(p.offset / sector).to_le_bytes());
    hd.extend_from_slice(&(p.size / sector).to_le_bytes());
    if p.style.eq_ignore_ascii_case("GPT") {
        hd.extend_from_slice(&guid_bytes(&p.guid)?);
        hd.extend_from_slice(&[2, 2]);
    } else {
        let mut sig = [0u8; 16];
        sig[..4].copy_from_slice(&p.signature?.to_le_bytes());
        hd.extend_from_slice(&sig);
        hd.extend_from_slice(&[1, 1]);
    }
    let file = utf16z(LOADER);
    let mut fp = vec![4u8, 4];
    fp.extend_from_slice(&((4 + file.len()) as u16).to_le_bytes());
    fp.extend_from_slice(&file);
    let end = [0x7Fu8, 0xFF, 4, 0];
    let list_len = (hd.len() + fp.len() + end.len()) as u16;
    let mut o = Vec::new();
    o.extend_from_slice(&1u32.to_le_bytes()); // LOAD_OPTION_ACTIVE
    o.extend_from_slice(&list_len.to_le_bytes());
    o.extend_from_slice(&utf16z(DESCRIPTION));
    o.extend(hd);
    o.extend(fp);
    o.extend_from_slice(&end);
    Some(o)
}

/// Wolny numer Boot#### albo numer istniejącego wpisu „Shrimp Install”.
fn boot_number() -> Option<u16> {
    let desc = utf16z(DESCRIPTION);
    let mut free = None;
    let mut buf = vec![0u8; 4096];
    for i in 0..0x1000u16 {
        let n = unsafe { GetFirmwareEnvironmentVariableW(&HSTRING::from(format!("Boot{i:04X}")), &HSTRING::from(EFI_GLOBAL), Some(buf.as_mut_ptr() as _), buf.len() as u32) } as usize;
        if n == 0 {
            free = free.or(Some(i));
            if i > 0x40 {
                break;
            }
            continue;
        }
        if n > 6 + desc.len() && buf[6..6 + desc.len()] == desc[..] {
            return Some(i);
        }
    }
    free
}

/// Ustawia start z nośnika przy następnym uruchomieniu. Błąd: kod dla interfejsu (noUefi, noPriv, noMedia, noSlot, fwEntry, fwNext).
pub fn set_boot_next(disk_number: u32) -> Result<(), &'static str> {
    if firmware() != "uefi" {
        return Err("noUefi");
    }
    if !privilege("SeSystemEnvironmentPrivilege") {
        return Err("noPriv");
    }
    let part = disk::boot_partition(disk_number).map_err(|e| {
        crate::log!("partycja startowa nośnika: {e}");
        "noMedia"
    })?;
    let opt = load_option(&part).ok_or("noMedia")?;
    let num = boot_number().ok_or("noSlot")?;
    let name = HSTRING::from(format!("Boot{num:04X}"));
    let guid = HSTRING::from(EFI_GLOBAL);
    unsafe {
        if let Err(e) = SetFirmwareEnvironmentVariableW(&name, &guid, Some(opt.as_ptr() as _), opt.len() as u32) {
            crate::log!("zapis {name} nie powiódł się: {e}");
            return Err("fwEntry");
        }
        let next = num.to_le_bytes();
        if let Err(e) = SetFirmwareEnvironmentVariableW(&HSTRING::from("BootNext"), &guid, Some(next.as_ptr() as _), 2) {
            crate::log!("zapis BootNext nie powiódł się: {e}");
            return Err("fwNext");
        }
    }
    crate::log!("BootNext = {name} (partycja {} nośnika)", part.number);
    Ok(())
}

/// Szybkie uruchamianie zostawia NTFS w stanie hibernacji, a wtedy Setup nie zmniejszy partycji Windows.
pub fn disable_fast_startup() {
    use winreg::{enums::*, RegKey};
    if let Ok(k) = RegKey::predef(HKEY_LOCAL_MACHINE).open_subkey_with_flags(r"SYSTEM\CurrentControlSet\Control\Session Manager\Power", KEY_SET_VALUE) {
        let _ = k.set_value("HiberbootEnabled", &0u32);
    }
}

pub fn reboot() -> bool {
    privilege("SeShutdownPrivilege");
    unsafe { ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OPERATINGSYSTEM | SHTDN_REASON_MINOR_INSTALLATION | SHTDN_REASON_FLAG_PLANNED).is_ok() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn efi_guid() {
        let g = guid_bytes("{12345678-9ABC-DEF0-1234-56789ABCDEF0}").unwrap();
        assert_eq!(g, [0x78, 0x56, 0x34, 0x12, 0xBC, 0x9A, 0xF0, 0xDE, 0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0]);
    }
}
