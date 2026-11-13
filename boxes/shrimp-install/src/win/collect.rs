//! To, co Setup Shrimp chce wiedzieć o Windows (shrimp.json): system, użytkownik, język, strefa,
//! klawiatury, programy. Prawie wszystko z rejestru; strefę IANA podaje ICU z Windows (icu.dll, 1903+).
use serde_json::{json, Value};
use std::collections::HashSet;
use windows::core::{s, HSTRING, PCWSTR};
use windows::Win32::NetworkManagement::NetManagement::{NetApiBufferFree, NetUserGetInfo, USER_INFO_2};
use windows::Win32::Foundation::FreeLibrary;
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryExW, LOAD_LIBRARY_SEARCH_SYSTEM32};
use winreg::enums::*;
use winreg::RegKey;

fn hkcu() -> RegKey {
    RegKey::predef(HKEY_CURRENT_USER)
}
fn hklm() -> RegKey {
    RegKey::predef(HKEY_LOCAL_MACHINE)
}
fn s_val(k: &RegKey, name: &str) -> Option<String> {
    k.get_value::<String, _>(name).ok().filter(|s| !s.is_empty())
}
fn opt(v: Option<String>) -> Value {
    v.map_or(Value::Null, Value::from)
}

fn windows_version() -> Option<String> {
    let k = hklm().open_subkey(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion").ok()?;
    let major: u32 = k.get_value("CurrentMajorVersionNumber").ok()?;
    let minor: u32 = k.get_value("CurrentMinorVersionNumber").unwrap_or(0);
    let build: String = k.get_value("CurrentBuildNumber").ok()?;
    Some(format!("{major}.{minor}.{build}"))
}

fn secure_boot() -> Option<bool> {
    let k = hklm().open_subkey(r"SYSTEM\CurrentControlSet\Control\SecureBoot\State").ok()?;
    k.get_value::<u32, _>("UEFISecureBootEnabled").ok().map(|v| v == 1)
}

fn full_name(user: &str) -> Option<String> {
    unsafe {
        let mut p: *mut u8 = std::ptr::null_mut();
        if NetUserGetInfo(PCWSTR::null(), &HSTRING::from(user), 2, &mut p) != 0 || p.is_null() {
            return None;
        }
        let ui = &*(p as *const USER_INFO_2);
        let r = if ui.usri2_full_name.is_null() { None } else { ui.usri2_full_name.to_string().ok() };
        let _ = NetApiBufferFree(Some(p as _));
        r.filter(|s| !s.is_empty())
    }
}

type UcalWin = unsafe extern "C" fn(*const u16, i32, *const u8, *mut u16, i32, *mut i32) -> i32;

fn iana(win: &str, region: Option<&str>) -> Option<String> {
    unsafe {
        let icu = LoadLibraryExW(&HSTRING::from("icu.dll"), None, LOAD_LIBRARY_SEARCH_SYSTEM32).ok()?;
        let r = GetProcAddress(icu, s!("ucal_getTimeZoneIDForWindowsID")).and_then(|f| {
            let f: UcalWin = std::mem::transmute(f);
            let w: Vec<u16> = win.encode_utf16().collect();
            let reg = region.map(|r| format!("{r}\0"));
            let mut out = [0u16; 128];
            let mut err = 0i32;
            let n = f(w.as_ptr(), w.len() as i32, reg.as_ref().map_or(std::ptr::null(), |r| r.as_ptr()), out.as_mut_ptr(), 127, &mut err);
            (err <= 0 && n > 0 && n < 128).then(|| String::from_utf16_lossy(&out[..n as usize]))
        });
        let _ = FreeLibrary(icu);
        r
    }
}

fn keyboards() -> Vec<String> {
    let Ok(pre) = hkcu().open_subkey(r"Keyboard Layout\Preload") else { return vec![] };
    let subst = hkcu().open_subkey(r"Keyboard Layout\Substitutes").ok();
    (1..32)
        .map_while(|i| s_val(&pre, &i.to_string()))
        .map(|klid| subst.as_ref().and_then(|s| s_val(s, &klid)).unwrap_or(klid))
        .filter(|k| k.len() == 8 && k.chars().all(|c| c.is_ascii_hexdigit()))
        .collect()
}

fn apps(seen: &mut HashSet<String>) -> Vec<Value> {
    let mut out = Vec::new();
    let roots = [(hklm(), KEY_WOW64_64KEY), (hklm(), KEY_WOW64_32KEY), (hkcu(), 0)];
    for (root, view) in roots {
        let Ok(k) = root.open_subkey_with_flags(r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall", KEY_READ | view) else { continue };
        for sub in k.enum_keys().flatten() {
            let Ok(s) = k.open_subkey_with_flags(&sub, KEY_READ | view) else { continue };
            let Some(name) = s_val(&s, "DisplayName") else { continue };
            let rel = s_val(&s, "ReleaseType").unwrap_or_default();
            if s.get_value::<u32, _>("SystemComponent").unwrap_or(0) == 1
                || s_val(&s, "ParentKeyName").is_some()
                || rel.contains("Update")
                || rel.contains("Hotfix")
                || name.starts_with("Update for")
                || name.starts_with("Security Update")
                || !seen.insert(name.to_lowercase())
            {
                continue;
            }
            out.push(json!({ "name": name, "publisher": opt(s_val(&s, "Publisher")), "version": opt(s_val(&s, "DisplayVersion")) }));
        }
    }
    out
}

/// Aplikacje ze Sklepu Microsoft: nazwy pakietów (np. SpotifyAB.SpotifyMusic), bez składników systemu.
fn store_apps(seen: &mut HashSet<String>) -> Vec<Value> {
    let Ok(k) = hkcu().open_subkey(r"Software\Classes\Local Settings\Software\Microsoft\Windows\CurrentVersion\AppModel\Repository\Packages") else { return vec![] };
    let keep_ms = ["Microsoft.Office", "Microsoft.Teams", "Microsoft.Skype", "Microsoft.Todos", "Microsoft.OutlookForWindows"];
    k.enum_keys()
        .flatten()
        .map(|s| s.split('_').next().unwrap_or("").to_string())
        .filter(|s| !s.is_empty())
        .filter(|s| !s.starts_with("Microsoft.") || keep_ms.iter().any(|m| s.starts_with(m)))
        .filter(|s| !s.starts_with("windows.") && !s.starts_with("Windows.") && !s.starts_with("MicrosoftWindows."))
        .filter(|s| seen.insert(s.to_lowercase()))
        .map(Value::from)
        .collect()
}

/// Domyślna przeglądarka Windows: ProgId z wyboru użytkownika dla https (np. „ChromeHTML”, „MSEdgeHTM”,
/// „FirefoxURL-308046B0AF4A39CB”). Onboarding Shrimp zaznacza jej odpowiednik w kroku „Przeglądarka”.
fn default_browser() -> Option<String> {
    ["https", "http"].iter().find_map(|scheme| {
        let k = hkcu().open_subkey(format!(r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations\{scheme}\UserChoice")).ok()?;
        s_val(&k, "ProgId")
    })
}

/// Pełny zbiór danych do shrimp.json (bez plików): windows, user, locale, apps, storeApps, defaultBrowser.
pub fn collect(bitlocker: bool) -> Value {
    let intl = hkcu().open_subkey(r"Control Panel\International").ok();
    let format = intl.as_ref().and_then(|k| s_val(k, "LocaleName"));
    let region = hkcu().open_subkey(r"Control Panel\International\Geo").ok().and_then(|k| s_val(&k, "Name")).filter(|r| r.len() == 2);
    let language = super::sys::ui_languages().into_iter().next();
    let win_tz = hklm().open_subkey(r"SYSTEM\CurrentControlSet\Control\TimeZoneInformation").ok().and_then(|k| s_val(&k, "TimeZoneKeyName"));
    let tz = win_tz.as_deref().and_then(|w| iana(w, region.as_deref()));
    let user = std::env::var("USERNAME").ok().filter(|s| !s.is_empty());
    let mut windows = json!({ "version": opt(windows_version()), "firmware": super::boot::firmware(), "bitlocker": bitlocker });
    if let Some(sb) = secure_boot() {
        windows["secureBoot"] = json!(sb);
    }
    let mut u = json!({ "name": opt(user.clone()) });
    if let Some(f) = user.as_deref().and_then(full_name) {
        u["fullName"] = json!(f);
    }
    let mut seen = HashSet::new();
    let mut out = json!({
        "windows": windows,
        "user": u,
        "locale": {
            "format": opt(format),
            "language": opt(language),
            "region": opt(region),
            "timezone": opt(tz),
            "windowsTimezone": opt(win_tz),
            "keyboards": keyboards(),
        },
        "apps": apps(&mut seen),
        "storeApps": store_apps(&mut seen),
    });
    if let Some(b) = default_browser() {
        out["defaultBrowser"] = json!(b);
    }
    out
}
