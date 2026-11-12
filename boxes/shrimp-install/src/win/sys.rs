//! Drobne wywołania Windows: foldery użytkownika, języki interfejsu, wolne miejsce, okno komunikatu, konsola.
use std::path::{Path, PathBuf};
use windows::core::{GUID, HSTRING, PCWSTR, PWSTR};
use windows::Win32::Globalization::{GetUserPreferredUILanguages, MUI_LANGUAGE_NAME};
use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
use windows::Win32::System::Com::CoTaskMemFree;
use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows::Win32::UI::Shell::{
    SHGetKnownFolderPath, FOLDERID_Desktop, FOLDERID_Documents, FOLDERID_Downloads, FOLDERID_Music, FOLDERID_Pictures, FOLDERID_Videos, KF_FLAG_DEFAULT,
};
use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONERROR, MB_OK};

fn known(id: &GUID) -> Option<PathBuf> {
    unsafe {
        let p = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let s = p.to_string().ok();
        CoTaskMemFree(Some(p.0 as *const _));
        s.map(PathBuf::from)
    }
}

/// Moje foldery w kolejności kluczy (transfer::FOLDER_KEYS); brakujące są pomijane.
pub fn folders() -> Vec<(String, PathBuf)> {
    let ids = [&FOLDERID_Documents, &FOLDERID_Pictures, &FOLDERID_Desktop, &FOLDERID_Music, &FOLDERID_Videos, &FOLDERID_Downloads];
    crate::transfer::FOLDER_KEYS.iter().zip(ids).filter_map(|(k, id)| known(id).map(|p| (k.to_string(), p))).collect()
}

pub fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir)
}

/// Języki interfejsu Windows w kolejności preferencji użytkownika (BCP-47).
pub fn ui_languages() -> Vec<String> {
    unsafe {
        let (mut n, mut len) = (0u32, 0u32);
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, None, &mut len).is_err() || len < 2 {
            return vec![];
        }
        let mut buf = vec![0u16; len as usize];
        if GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &mut n, Some(PWSTR(buf.as_mut_ptr())), &mut len).is_err() {
            return vec![];
        }
        buf.split(|c| *c == 0).filter(|s| !s.is_empty()).map(String::from_utf16_lossy).collect()
    }
}

pub fn free_space(p: &Path) -> Option<u64> {
    let mut free = 0u64;
    let dir = if p.is_dir() { p.to_path_buf() } else { p.parent()?.to_path_buf() };
    unsafe { GetDiskFreeSpaceExW(&HSTRING::from(dir.as_os_str()), Some(&mut free), None, None).ok().map(|_| free) }
}

pub fn message(text: &str) {
    unsafe {
        MessageBoxW(None, &HSTRING::from(text), &HSTRING::from("Shrimp Install"), MB_OK | MB_ICONERROR);
    }
}

/// Tryby bez okna (--collect, --backup-test) piszą do konsoli, z której je uruchomiono.
pub fn attach_console() {
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[allow(dead_code)]
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

#[allow(dead_code)]
pub fn pcwstr(v: &[u16]) -> PCWSTR {
    PCWSTR(v.as_ptr())
}
