/*
 * Shrimp Install: to, co Setup Shrimp chce wiedzieć o Windows (shrimp.json):
 * programy, użytkownik, język, strefa czasowa, klawiatura, BitLocker, foldery.
 * Copyright © 2026 Shrimp. GPLv3.
 */
#define COBJMACROS
#include <windows.h>
#include <shlobj.h>
#include <lm.h>
#include <lmerr.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>
#include "shrimp.h"
#include "../rufus.h"
#include "../msapi_utf8.h"

const char* shrimp_folder_key[SF_MAX] = { "documents", "pictures", "desktop", "music", "videos", "downloads" };
static const KNOWNFOLDERID* folder_id[SF_MAX] = {
	&FOLDERID_Documents, &FOLDERID_Pictures, &FOLDERID_Desktop, &FOLDERID_Music, &FOLDERID_Videos, &FOLDERID_Downloads
};

BOOL ShrimpFolderPath(shrimp_folder f, wchar_t* path, size_t size)
{
	PWSTR p = NULL;
	BOOL r = FALSE;
	if (f < 0 || f >= SF_MAX)
		return FALSE;
	if (SUCCEEDED(SHGetKnownFolderPath(folder_id[f], 0, NULL, &p)) && p != NULL) {
		r = (wcslen(p) < size);
		if (r)
			wcscpy(path, p);
	}
	CoTaskMemFree(p);
	return r;
}

#ifndef FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
#define FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS 0x00400000
#endif
#ifndef FILE_ATTRIBUTE_RECALL_ON_OPEN
#define FILE_ATTRIBUTE_RECALL_ON_OPEN 0x00040000
#endif
#define SHRIMP_CLOUD_ONLY (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN)

uint64_t ShrimpTreeSize(const wchar_t* path, volatile BOOL* cancel)
{
	WIN32_FIND_DATAW fd;
	wchar_t* pat;
	HANDLE h;
	uint64_t total = 0;
	size_t n = wcslen(path) + 8;
	DWORD a = GetFileAttributesW(path);

	if (a == INVALID_FILE_ATTRIBUTES)
		return 0;
	if (!(a & FILE_ATTRIBUTE_DIRECTORY)) {
		WIN32_FILE_ATTRIBUTE_DATA d;
		if ((a & SHRIMP_CLOUD_ONLY) || !GetFileAttributesExW(path, GetFileExInfoStandard, &d))
			return 0;
		return ((uint64_t)d.nFileSizeHigh << 32) | d.nFileSizeLow;
	}
	pat = malloc(n * sizeof(wchar_t));
	if (pat == NULL)
		return 0;
	swprintf(pat, n, L"%s\\*", path);
	h = FindFirstFileExW(pat, FindExInfoBasic, &fd, FindExSearchNameMatch, NULL, FIND_FIRST_EX_LARGE_FETCH);
	free(pat);
	if (h == INVALID_HANDLE_VALUE)
		return 0;
	do {
		if (cancel != NULL && *cancel)
			break;
		if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0)
			continue;
		/* Dowiązania (np. stare „Moja muzyka” w Dokumentach) i pliki tylko w chmurze pomijamy. */
		if (fd.dwFileAttributes & (FILE_ATTRIBUTE_REPARSE_POINT | SHRIMP_CLOUD_ONLY))
			continue;
		if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
			size_t m = wcslen(path) + wcslen(fd.cFileName) + 2;
			wchar_t* sub = malloc(m * sizeof(wchar_t));
			if (sub == NULL)
				continue;
			swprintf(sub, m, L"%s\\%s", path, fd.cFileName);
			total += ShrimpTreeSize(sub, cancel);
			free(sub);
		} else {
			total += ((uint64_t)fd.nFileSizeHigh << 32) | fd.nFileSizeLow;
		}
	} while (FindNextFileW(h, &fd));
	FindClose(h);
	return total;
}

/* ---------- BitLocker ---------- */
/*
 * Get-BitLockerVolume zwraca ProtectionStatus „On” albo „Off” niezależnie od języka Windows.
 * Bez modułu BitLocker (lub przy błędzie) przyjmujemy, że dysk nie jest zaszyfrowany.
 */
BOOL ShrimpBitLocker(void)
{
	char cmd[1024];
	static_sprintf(cmd, "%s\\WindowsPowerShell\\v1.0\\powershell.exe -NoProfile -NonInteractive -WindowStyle Hidden -Command "
		"\"try { if ((Get-BitLockerVolume -MountPoint $env:SystemDrive -ErrorAction Stop).ProtectionStatus -eq 'On') { exit 2 } "
		"else { exit 3 } } catch { exit 4 }\"", system_dir);
	DWORD r = RunCommand(cmd, NULL, FALSE);
	uprintf("Shrimp: BitLocker %s (%lu)", r == 2 ? "włączony" : "wyłączony albo nieznany", r);
	return r == 2;
}

/* ---------- rejestr ---------- */
static BOOL reg_wstr(HKEY k, const wchar_t* name, wchar_t* out, DWORD size_chars)
{
	DWORD type, size = size_chars * sizeof(wchar_t);
	if (RegQueryValueExW(k, name, NULL, &type, (BYTE*)out, &size) != ERROR_SUCCESS)
		return FALSE;
	if (type != REG_SZ && type != REG_EXPAND_SZ)
		return FALSE;
	out[min(size / sizeof(wchar_t), size_chars - 1)] = 0;
	return out[0] != 0;
}

static DWORD reg_dword(HKEY k, const wchar_t* name, DWORD def)
{
	DWORD v, type, size = sizeof(v);
	if (RegQueryValueExW(k, name, NULL, &type, (BYTE*)&v, &size) != ERROR_SUCCESS || type != REG_DWORD)
		return def;
	return v;
}

/* ---------- programy ---------- */
#define MAX_APPS 1024
static wchar_t* seen[MAX_APPS];
static int nseen;

static BOOL seen_before(const wchar_t* name)
{
	for (int i = 0; i < nseen; i++)
		if (_wcsicmp(seen[i], name) == 0)
			return TRUE;
	if (nseen < MAX_APPS)
		seen[nseen++] = _wcsdup(name);
	return FALSE;
}

static void apps_from(jb* b, HKEY root, REGSAM view)
{
	HKEY k, s;
	wchar_t sub[256], name[512], pub[256], ver[128], rel[64];
	if (RegOpenKeyExW(root, L"SOFTWARE\\Microsoft\\Windows\\CurrentVersion\\Uninstall", 0, KEY_READ | view, &k) != ERROR_SUCCESS)
		return;
	for (DWORD i = 0; ; i++) {
		DWORD n = ARRAYSIZE(sub);
		if (RegEnumKeyExW(k, i, sub, &n, NULL, NULL, NULL, NULL) != ERROR_SUCCESS)
			break;
		if (RegOpenKeyExW(k, sub, 0, KEY_READ | view, &s) != ERROR_SUCCESS)
			continue;
		if (reg_wstr(s, L"DisplayName", name, ARRAYSIZE(name))
			&& reg_dword(s, L"SystemComponent", 0) != 1
			&& !reg_wstr(s, L"ParentKeyName", rel, ARRAYSIZE(rel))
			&& !(reg_wstr(s, L"ReleaseType", rel, ARRAYSIZE(rel)) && (wcsstr(rel, L"Update") || wcsstr(rel, L"Hotfix")))
			&& wcsncmp(name, L"Update for", 10) != 0 && wcsncmp(name, L"Security Update", 15) != 0
			&& !seen_before(name)) {
			jb_obj(b, NULL);
			jb_wstr(b, "name", name);
			jb_wstr(b, "publisher", reg_wstr(s, L"Publisher", pub, ARRAYSIZE(pub)) ? pub : NULL);
			jb_wstr(b, "version", reg_wstr(s, L"DisplayVersion", ver, ARRAYSIZE(ver)) ? ver : NULL);
			jb_end_obj(b);
		}
		RegCloseKey(s);
	}
	RegCloseKey(k);
}

/* Aplikacje ze Sklepu Microsoft: nazwy pakietów (np. SpotifyAB.SpotifyMusic). */
static void store_apps(jb* b)
{
	HKEY k;
	wchar_t sub[512];
	if (RegOpenKeyExW(HKEY_CURRENT_USER, L"Software\\Classes\\Local Settings\\Software\\Microsoft\\Windows\\CurrentVersion\\AppModel\\Repository\\Packages",
		0, KEY_READ, &k) != ERROR_SUCCESS)
		return;
	for (DWORD i = 0; ; i++) {
		DWORD n = ARRAYSIZE(sub);
		if (RegEnumKeyExW(k, i, sub, &n, NULL, NULL, NULL, NULL) != ERROR_SUCCESS)
			break;
		wchar_t* u = wcschr(sub, L'_');
		if (u != NULL)
			*u = 0;
		/* Składniki systemu pomijamy. */
		if (wcsncmp(sub, L"Microsoft.", 10) == 0 && wcsncmp(sub, L"Microsoft.Office", 16) != 0
			&& wcsncmp(sub, L"Microsoft.Teams", 15) != 0 && wcsncmp(sub, L"Microsoft.Skype", 15) != 0
			&& wcsncmp(sub, L"Microsoft.Todos", 15) != 0 && wcsncmp(sub, L"Microsoft.OutlookForWindows", 27) != 0)
			continue;
		if (wcsncmp(sub, L"windows.", 8) == 0 || wcsncmp(sub, L"Windows.", 8) == 0 || wcsncmp(sub, L"MicrosoftWindows.", 17) == 0)
			continue;
		if (!seen_before(sub))
			jb_wstr(b, NULL, sub);
	}
	RegCloseKey(k);
}

/* ---------- język, strefa, klawiatura ---------- */
typedef int32_t (WINAPI *ucal_win_t)(const wchar_t*, int32_t, const char*, wchar_t*, int32_t, int*);

static void timezone_iana(const wchar_t* win, const char* region, wchar_t* out, int size)
{
	HMODULE icu = LoadLibraryExW(L"icu.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
	out[0] = 0;
	if (icu == NULL)
		return;
	ucal_win_t f = (ucal_win_t)(void*)GetProcAddress(icu, "ucal_getTimeZoneIDForWindowsID");
	if (f != NULL) {
		int err = 0;
		int32_t n = f(win, -1, region, out, size - 1, &err);
		out[(err <= 0 && n > 0 && n < size) ? n : 0] = 0;
	}
	FreeLibrary(icu);
}

static void keyboards(jb* b)
{
	HKEY pre, subst;
	wchar_t klid[KL_NAMELENGTH + 1], name[16], s[KL_NAMELENGTH + 1];
	jb_arr(b, "keyboards");
	if (RegOpenKeyExW(HKEY_CURRENT_USER, L"Keyboard Layout\\Preload", 0, KEY_READ, &pre) == ERROR_SUCCESS) {
		BOOL has_subst = RegOpenKeyExW(HKEY_CURRENT_USER, L"Keyboard Layout\\Substitutes", 0, KEY_READ, &subst) == ERROR_SUCCESS;
		for (int i = 1; i < 32; i++) {
			swprintf(name, ARRAYSIZE(name), L"%d", i);
			if (!reg_wstr(pre, name, klid, ARRAYSIZE(klid)))
				break;
			if (has_subst && reg_wstr(subst, klid, s, ARRAYSIZE(s)))
				wcscpy(klid, s);
			jb_wstr(b, NULL, klid);
		}
		if (has_subst)
			RegCloseKey(subst);
		RegCloseKey(pre);
	}
	jb_end_arr(b);
}

static void user_names(jb* b)
{
	wchar_t user[UNLEN + 1] = L"";
	DWORD n = ARRAYSIZE(user);
	jb_obj(b, "user");
	if (GetUserNameW(user, &n))
		jb_wstr(b, "name", user);
	/* Pełna nazwa: konto lokalne i konto Microsoft trzymają ją w usri2_full_name. */
	HMODULE net = LoadLibraryExW(L"netapi32.dll", NULL, LOAD_LIBRARY_SEARCH_SYSTEM32);
	typedef NET_API_STATUS (WINAPI *get_t)(LPCWSTR, LPCWSTR, DWORD, LPBYTE*);
	typedef NET_API_STATUS (WINAPI *free_t)(LPVOID);
	if (net != NULL) {
		get_t get = (get_t)(void*)GetProcAddress(net, "NetUserGetInfo");
		free_t fr = (free_t)(void*)GetProcAddress(net, "NetApiBufferFree");
		USER_INFO_2* ui = NULL;
		if (get && fr && get(NULL, user, 2, (LPBYTE*)&ui) == NERR_Success && ui != NULL) {
			if (ui->usri2_full_name && ui->usri2_full_name[0])
				jb_wstr(b, "fullName", ui->usri2_full_name);
			fr(ui);
		}
		FreeLibrary(net);
	}
	jb_end_obj(b);
}

void ShrimpCollect(jb* b)
{
	wchar_t loc[LOCALE_NAME_MAX_LENGTH] = L"", ui[LOCALE_NAME_MAX_LENGTH] = L"", geo[8] = L"", tz[128] = L"";
	char region[8] = "";
	DYNAMIC_TIME_ZONE_INFORMATION dtz;
	FIRMWARE_TYPE fw = FirmwareTypeUnknown;
	HKEY k;
	char ver[64];

	jb_obj(b, "windows");
	static_sprintf(ver, "%d.%d.%d", WindowsVersion.Major, WindowsVersion.Minor, WindowsVersion.BuildNumber);
	jb_str(b, "version", ver);
	GetFirmwareType(&fw);
	jb_str(b, "firmware", fw == FirmwareTypeUefi ? "uefi" : fw == FirmwareTypeBios ? "bios" : "unknown");
	if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SYSTEM\\CurrentControlSet\\Control\\SecureBoot\\State", 0, KEY_READ, &k) == ERROR_SUCCESS) {
		jb_bool(b, "secureBoot", reg_dword(k, L"UEFISecureBootEnabled", 0) == 1);
		RegCloseKey(k);
	}
	jb_bool(b, "bitlocker", ShrimpBitLockerOn);
	jb_end_obj(b);

	user_names(b);

	jb_obj(b, "locale");
	GetUserDefaultLocaleName(loc, ARRAYSIZE(loc));
	jb_wstr(b, "format", loc);
	LCIDToLocaleName(MAKELCID(GetUserDefaultUILanguage(), SORT_DEFAULT), ui, ARRAYSIZE(ui), 0);
	jb_wstr(b, "language", ui);
	if (GetGeoInfoW(GetUserGeoID(GEOCLASS_NATION), GEO_ISO2, geo, ARRAYSIZE(geo), 0) > 0) {
		jb_wstr(b, "region", geo);
		WideCharToMultiByte(CP_UTF8, 0, geo, -1, region, sizeof(region), NULL, NULL);
	}
	if (GetDynamicTimeZoneInformation(&dtz) != TIME_ZONE_ID_INVALID) {
		jb_wstr(b, "windowsTimezone", dtz.TimeZoneKeyName);
		timezone_iana(dtz.TimeZoneKeyName, region[0] ? region : NULL, tz, ARRAYSIZE(tz));
		jb_wstr(b, "timezone", tz[0] ? tz : NULL);
	}
	keyboards(b);
	jb_end_obj(b);

	nseen = 0;
	jb_arr(b, "apps");
	apps_from(b, HKEY_LOCAL_MACHINE, KEY_WOW64_64KEY);
	apps_from(b, HKEY_LOCAL_MACHINE, KEY_WOW64_32KEY);
	apps_from(b, HKEY_CURRENT_USER, 0);
	jb_end_arr(b);
	jb_arr(b, "storeApps");
	store_apps(b);
	jb_end_arr(b);
	for (int i = 0; i < nseen; i++)
		free(seen[i]);
	nseen = 0;
}
