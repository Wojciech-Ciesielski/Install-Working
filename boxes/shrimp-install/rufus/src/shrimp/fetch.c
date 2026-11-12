/*
 * Shrimp Install: pobieranie obrazu Shrimp.
 *
 * Manifest (JSON, mały) mówi, skąd pobrać ISO, ile ma bajtów i jaka jest jego suma SHA-256:
 *   { "version": "Beta 3.5", "url": "https://…/Shrimp.x86_64-0.1.0.iso", "size": 3650722816, "sha256": "…" }
 * Obok leży podpis manifestu (manifest.json.sig, RSA-2048 + SHA-256, `openssl dgst -sha256 -sign`).
 * Klucz publiczny jest wkompilowany (shrimp_key.h). Bez klucza (kompilacja deweloperska)
 * zaufanie do manifestu opiera się tylko na HTTPS, a suma SHA-256 ISO jest sprawdzana zawsze.
 *
 * Copyright © 2026 Shrimp. GPLv3.
 */
#include <windows.h>
#include <wininet.h>
#include <shlobj.h>
#include <stdio.h>
#include <string.h>
#include <ctype.h>
#include "shrimp.h"
#include "shrimp_key.h"
#include "relurl.h"
#include "../rufus.h"
#include "../msapi_utf8.h"

/* Adres manifestu daje tools/prepare.sh (shrimp_config.h) według repozytorium, z którego budujemy:
 *  - repozytorium prywatne: https://api.github.com/repos/<repo>/releases/latest (potrzebny token, patrz niżej),
 *  - repozytorium publiczne: https://github.com/<repo>/releases/latest/download/manifest.json. */
#if defined(__has_include)
#if __has_include("shrimp_config.h")
#include "shrimp_config.h"
#endif
#endif
#ifndef SHRIMP_MANIFEST_URL
#define SHRIMP_MANIFEST_URL "https://download.shrimp.example/install/manifest.json"
#endif

shrimp_manifest ShrimpManifest;
char ShrimpIsoPath[MAX_PATH];
char ShrimpLocalIso[MAX_PATH];
char ShrimpManifestOverride[1024];
volatile BOOL ShrimpCancel;

extern BOOL ValidateOpensslSignatureWithKey(BYTE* pbBuffer, DWORD dwBufferLen, BYTE* pbSignature, DWORD dwSigLen,
	const uint8_t* modulus, size_t modulus_len);

const char* ShrimpManifestUrl(void)
{
	return ShrimpManifestOverride[0] ? ShrimpManifestOverride : SHRIMP_MANIFEST_URL;
}

/* Ścieżki są w UTF-8 (jak w Rufusie). */
static BOOL attrs_u(const char* path, WIN32_FILE_ATTRIBUTE_DATA* d)
{
	wchar_t* w = utf8_to_wchar(path);
	BOOL r = w != NULL && GetFileAttributesExW(w, GetFileExInfoStandard, d);
	free(w);
	return r;
}

static BOOL free_space_u(const char* dir, ULARGE_INTEGER* free_bytes)
{
	wchar_t* w = utf8_to_wchar(dir);
	BOOL r = w != NULL && GetDiskFreeSpaceExW(w, free_bytes, NULL, NULL);
	free(w);
	return r;
}

static BOOL offline_error(DWORD e)
{
	return e == ERROR_INTERNET_NAME_NOT_RESOLVED || e == ERROR_INTERNET_CANNOT_CONNECT || e == ERROR_INTERNET_TIMEOUT
		|| e == ERROR_INTERNET_CONNECTION_ABORTED || e == ERROR_INTERNET_CONNECTION_RESET || e == ERROR_INTERNET_DISCONNECTED
		|| e == ERROR_INTERNET_SERVER_UNREACHABLE || e == ERROR_INTERNET_PROXY_SERVER_UNREACHABLE;
}

static BOOL cache_dir(char* dir, size_t size);

/* Pobieranie ISO ponawia się po zerwaniu połączenia. W czasie prób, które mogą się jeszcze powtórzyć, błędy sieci
 * są tylko zapamiętywane (net_quiet); interfejs dostaje błąd dopiero po ostatniej próbie. */
static BOOL net_quiet, net_fatal;
static const char* net_kind;
static char net_msg[160];

static void net_fail(const char* kind, const char* msg)
{
	if (!net_quiet) {
		ShrimpFail(1, kind, msg);
		return;
	}
	net_kind = kind;
	static_strcpy(net_msg, msg ? msg : "");
	/* Błąd serwera (404, przekierowanie, brak tokenu) nie minie po ponowieniu. */
	net_fatal = strcmp(kind, "offline") != 0;
}

#define API_HOST "https://api.github.com/"
#define ACCEPT_JSON 1
#define ACCEPT_BINARY 0

/* Token do prywatnego repozytorium: zmienna SHRIMP_TOKEN albo pierwszy wiersz pliku
 * %LOCALAPPDATA%\Shrimp Install\token.txt. Nigdy nie trafia do dziennika. Wysyłamy go tylko do api.github.com. */
static const char* shrimp_token(void)
{
	static char token[256];
	static BOOL done;
	if (!done) {
		char dir[MAX_PATH], path[MAX_PATH], *p;
		FILE* f;
		done = TRUE;
		if (GetEnvironmentVariableA("SHRIMP_TOKEN", token, sizeof(token)) == 0 || strlen(token) >= sizeof(token) - 1)
			token[0] = 0;
		if (!token[0] && cache_dir(dir, sizeof(dir))) {
			static_sprintf(path, "%s\\token.txt", dir);
			f = fopen(path, "r");
			if (f != NULL) {
				if (fgets(token, sizeof(token), f) == NULL)
					token[0] = 0;
				fclose(f);
			}
		}
		for (p = token; *p; p++)
			if (*p == '\r' || *p == '\n' || *p == ' ') {
				*p = 0;
				break;
			}
	}
	return token;
}

/* Otwiera adres (ręcznie idąc za przekierowaniami, żeby token nie wyciekł poza api.github.com);
 * przy błędzie zgłasza go interfejsowi i zwraca NULL. */
static HINTERNET open_url(HINTERNET session, const char* url, uint64_t* length, int accept_json, uint64_t from, DWORD* status_out)
{
	char buf[32], cur[1100], hdr[700], loc[1100];
	DWORD status = 0, size = sizeof(status);
	HINTERNET r = NULL;
	int hop;

	static_strcpy(cur, url);
	for (hop = 0; hop < 6; hop++) {
		BOOL api = strncmp(cur, API_HOST, sizeof(API_HOST) - 1) == 0;
		const char* tok = api ? shrimp_token() : "";
		static_strcpy(hdr, "Accept-Encoding: identity\r\n");
		if (from > 0) {
			/* Wznowienie: reszta pliku od bajtu `from` (serwer odpowiada 206, a 200 = cały plik od nowa). */
			char range[64];
			static_sprintf(range, "Range: bytes=%llu-\r\n", (unsigned long long)from);
			strncat(hdr, range, sizeof(hdr) - strlen(hdr) - 1);
		}
		if (api) {
			strncat(hdr, accept_json ? "Accept: application/vnd.github+json\r\n" : "Accept: application/octet-stream\r\n",
				sizeof(hdr) - strlen(hdr) - 1);
			if (tok[0]) {
				strncat(hdr, "Authorization: Bearer ", sizeof(hdr) - strlen(hdr) - 1);
				strncat(hdr, tok, sizeof(hdr) - strlen(hdr) - 1);
				strncat(hdr, "\r\n", sizeof(hdr) - strlen(hdr) - 1);
			}
		}
		r = InternetOpenUrlA(session, cur, hdr, (DWORD)-1,
			INTERNET_FLAG_NO_CACHE_WRITE | INTERNET_FLAG_NO_UI | INTERNET_FLAG_NO_COOKIES | INTERNET_FLAG_RELOAD
				| INTERNET_FLAG_NO_AUTO_REDIRECT, 0);
		if (r == NULL) {
			DWORD e = GetLastError();
			uprintf("Shrimp: %s: błąd %lu", cur, e);
			if (offline_error(e))
				net_fail("offline", NULL);
			else
				net_fail("other", "connect");
			return NULL;
		}
		status = 0;
		size = sizeof(status);
		HttpQueryInfoA(r, HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER, &status, &size, NULL);
		if (status == 301 || status == 302 || status == 303 || status == 307 || status == 308) {
			size = sizeof(loc);
			if (!HttpQueryInfoA(r, HTTP_QUERY_LOCATION, loc, &size, NULL) || strncmp(loc, "https://", 8) != 0) {
				InternetCloseHandle(r);
				net_fail("other", "redirect");
				return NULL;
			}
			InternetCloseHandle(r);
			r = NULL;
			static_strcpy(cur, loc);
			continue;
		}
		break;
	}
	if (r == NULL) {
		net_fail("other", "redirect");
		return NULL;
	}
	if (status_out != NULL)
		*status_out = status;
	if (status != 200 && !(status == 206 && from > 0)) {
		char m[160];
		uprintf("Shrimp: %s: HTTP %lu", cur, status);
		if ((status == 404 || status == 401) && strncmp(cur, API_HOST, sizeof(API_HOST) - 1) == 0 && !shrimp_token()[0])
			static_strcpy(m, "Prywatne repozytorium: ustaw zmienną SHRIMP_TOKEN (token GitHub z dostępem do odczytu).");
		else
			static_sprintf(m, "http:%lu", status);
		net_fail("other", m);
		InternetCloseHandle(r);
		return NULL;
	}
	*length = 0;
	size = sizeof(buf);
	if (HttpQueryInfoA(r, HTTP_QUERY_CONTENT_LENGTH, buf, &size, NULL))
		*length = strtoull(buf, NULL, 10);
	return r;
}

/* Mały plik do pamięci (manifest, podpis). */
static BYTE* fetch_small(HINTERNET session, const char* url, DWORD* out_len, int accept_json)
{
	uint64_t len;
	DWORD n, total = 0, cap = 256 * 1024;
	BYTE* buf;
	HINTERNET r = open_url(session, url, &len, accept_json, 0, NULL);
	if (r == NULL)
		return NULL;
	buf = calloc(1, cap + 1);
	while (buf != NULL && InternetReadFile(r, buf + total, cap - total, &n) && n > 0) {
		total += n;
		if (total >= cap) {
			ShrimpFail(1, "other", "manifestBig");
			free(buf);
			buf = NULL;
		}
	}
	InternetCloseHandle(r);
	if (buf != NULL)
		*out_len = total;
	return buf;
}

static BOOL hex_equal(const uint8_t* digest, const char* hex)
{
	char h[65];
	for (int i = 0; i < 32; i++)
		sprintf(&h[i * 2], "%02x", digest[i]);
	return _stricmp(h, hex) == 0;
}

/* Liczy SHA-256 istniejącego pliku (gdy ISO jest już pobrane). */
static BOOL hash_file(const char* path, uint64_t size, uint8_t* digest)
{
	HASH_CONTEXT ctx;
	static uint8_t buf[4 * 1024 * 1024];
	DWORD n;
	uint64_t done = 0;
	HANDLE h = CreateFileU(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, NULL);
	if (h == INVALID_HANDLE_VALUE)
		return FALSE;
	hash_init[HASH_SHA256](&ctx);
	ShrimpSpeed(0, TRUE);
	while (!ShrimpCancel && ReadFile(h, buf, sizeof(buf), &n, NULL) && n > 0) {
		hash_write[HASH_SHA256](&ctx, buf, n);
		done += n;
		ShrimpProgressStep(1, "verify", done, size, ShrimpSpeed(done, FALSE));
	}
	CloseHandle(h);
	hash_final[HASH_SHA256](&ctx);
	memcpy(digest, ctx.buf, 32);
	return !ShrimpCancel && done == size;
}

static BOOL cache_dir(char* dir, size_t size)
{
	PWSTR p = NULL;
	char* u;
	if (FAILED(SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, NULL, &p)))
		return FALSE;
	u = wchar_to_utf8(p);
	CoTaskMemFree(p);
	if (u == NULL)
		return FALSE;
	safe_sprintf(dir, size, "%s\\Shrimp Install", u);
	free(u);
	CreateDirectoryU(dir, NULL);
	return TRUE;
}

#define FETCH_ATTEMPTS 4

static uint64_t file_size_u(const char* path)
{
	WIN32_FILE_ATTRIBUTE_DATA d;
	return attrs_u(path, &d) ? (((uint64_t)d.nFileSizeHigh << 32) | d.nFileSizeLow) : 0;
}

/* Usuwa pliki shrimp-*<ext> z katalogu pobierania poza `keep`: części starych pobrań i obrazy
 * starszych wydań (np. shrimp-Beta-2.iso, 2 GB, zostawał obok nowego obrazu). */
static void remove_stale(const char* dir, const char* ext, const char* keep)
{
	char pattern[MAX_PATH], path[MAX_PATH];
	WIN32_FIND_DATAW fd;
	HANDLE h;
	wchar_t* w;
	static_sprintf(pattern, "%s\\shrimp-*%s", dir, ext);
	w = utf8_to_wchar(pattern);
	if (w == NULL)
		return;
	h = FindFirstFileW(w, &fd);
	free(w);
	if (h == INVALID_HANDLE_VALUE)
		return;
	do {
		char* name = wchar_to_utf8(fd.cFileName);
		if (name != NULL) {
			static_sprintf(path, "%s\\%s", dir, name);
			size_t nl = strlen(name), el = strlen(ext);
			/* FindFirstFile dopasowuje też krótkie nazwy 8.3: tylko dokładne rozszerzenie. */
			if (nl > el && _stricmp(name + nl - el, ext) == 0 && _stricmp(path, keep) != 0 && DeleteFileU(path))
				uprintf("Shrimp: usunięto stary plik pobierania %s", name);
			free(name);
		}
	} while (FindNextFileW(h, &fd));
	FindClose(h);
}

DWORD WINAPI ShrimpFetchThread(LPVOID param)
{
	HINTERNET session = NULL, r = NULL;
	BYTE *manifest = NULL, *sig = NULL;
	DWORD mlen = 0, slen = 0, rlen = 0, n, w;
	char murl[1100], sig_url[1100], tmp[64], dir[MAX_PATH], part[MAX_PATH], name[96];
	uint8_t digest[32];
	uint64_t len = 0, done = 0;
	ULARGE_INTEGER free_bytes;
	HANDLE f = INVALID_HANDLE_VALUE;
	static uint8_t buf[1024 * 1024];
	BOOL ok = FALSE;

	(void)param;
	/* Obraz wskazany ręcznie (--source): bez pobierania i bez sprawdzania sumy. */
	if (ShrimpLocalIso[0]) {
		WIN32_FILE_ATTRIBUTE_DATA d;
		if (!attrs_u(ShrimpLocalIso, &d)) {
			ShrimpFail(1, "other", "Nie znaleziono wskazanego obrazu.");
			goto out;
		}
		static_strcpy(ShrimpIsoPath, ShrimpLocalIso);
		ShrimpManifest.size = ((uint64_t)d.nFileSizeHigh << 32) | d.nFileSizeLow;
		ShrimpEmitImage();
		ShrimpProgress(1, ShrimpManifest.size, ShrimpManifest.size, 0);
		ok = TRUE;
		goto out;
	}

	session = InternetOpenA("ShrimpInstall/" SHRIMP_VERSION, INTERNET_OPEN_TYPE_PRECONFIG, NULL, NULL, 0);
	if (session == NULL) {
		ShrimpFail(1, "offline", NULL);
		goto out;
	}
	ShrimpProgress(1, 0, 0, 0);
	static_strcpy(murl, ShrimpManifestUrl());
	static_sprintf(sig_url, "%s.sig", murl);
	/* Adres wydania GitHub (API): manifest i podpis są zasobami tego wydania. */
	if (strncmp(murl, API_HOST, sizeof(API_HOST) - 1) == 0 && strstr(murl, "/releases/") != NULL && strstr(murl, "/releases/assets/") == NULL) {
		BYTE* rel = fetch_small(session, murl, &rlen, ACCEPT_JSON);
		if (rel == NULL)
			goto out;
		if (!shrimp_release_asset_url((char*)rel, "manifest.json", murl, sizeof(murl))) {
			free(rel);
			ShrimpFail(1, "other", "To wydanie nie ma pliku manifest.json.");
			goto out;
		}
		if (!shrimp_release_asset_url((char*)rel, "manifest.json.sig", sig_url, sizeof(sig_url)))
			sig_url[0] = 0;
		free(rel);
	}
	manifest = fetch_small(session, murl, &mlen, ACCEPT_BINARY);
	if (manifest == NULL)
		goto out;
	if (SHRIMP_PUBKEY_SET) {
		if (!sig_url[0]) {
			ShrimpFail(1, "verify", "sigBad");
			goto out;
		}
		sig = fetch_small(session, sig_url, &slen, ACCEPT_BINARY);
		if (sig == NULL)
			goto out;
		if (!ValidateOpensslSignatureWithKey(manifest, mlen, sig, slen, shrimp_pubkey_modulus, sizeof(shrimp_pubkey_modulus))) {
			ShrimpFail(1, "verify", "sigBad");
			goto out;
		}
	} else {
		uprintf("Shrimp: UWAGA: kompilacja bez klucza, podpis manifestu nie jest sprawdzany");
	}
	memset(&ShrimpManifest, 0, sizeof(ShrimpManifest));
	json_get((char*)manifest, "version", ShrimpManifest.version, sizeof(ShrimpManifest.version));
	json_get((char*)manifest, "url", ShrimpManifest.url, sizeof(ShrimpManifest.url));
	json_get((char*)manifest, "sha256", ShrimpManifest.sha256, sizeof(ShrimpManifest.sha256));
	if (json_get((char*)manifest, "size", tmp, sizeof(tmp)))
		ShrimpManifest.size = strtoull(tmp, NULL, 10);
	if (!ShrimpManifest.url[0] || strlen(ShrimpManifest.sha256) != 64 || ShrimpManifest.size == 0) {
		ShrimpFail(1, "other", "Manifest pobierania jest niekompletny.");
		goto out;
	}
	uprintf("Shrimp: %s, %s (%llu B)", ShrimpManifest.version, ShrimpManifest.url, ShrimpManifest.size);
	ShrimpEmitImage();

	if (!cache_dir(dir, sizeof(dir))) {
		ShrimpFail(1, "other", "tempAccess");
		goto out;
	}
	/* Nazwa pliku z wersji, tylko bezpieczne znaki. */
	static_strcpy(name, "shrimp-");
	{
		size_t k = strlen(name);
		for (char* v = ShrimpManifest.version; *v && k < sizeof(name) - 6; v++)
			name[k++] = (char)(isalnum((unsigned char)*v) || *v == '.' || *v == '-' ? *v : '-');
		name[k] = 0;
	}
	static_sprintf(ShrimpIsoPath, "%s\\%s.iso", dir, name);
	/* Część pobrana wcześniej należy do konkretnego obrazu (suma w nazwie), więc nowe wydanie nie dokleja się do starego. */
	static_sprintf(part, "%s.%.16s.part", ShrimpIsoPath, ShrimpManifest.sha256);
	remove_stale(dir, ".part", part);
	remove_stale(dir, ".iso", ShrimpIsoPath);

	/* Już pobrane? Wystarczy sprawdzić sumę. */
	{
		WIN32_FILE_ATTRIBUTE_DATA d;
		if (attrs_u(ShrimpIsoPath, &d)
			&& ((((uint64_t)d.nFileSizeHigh << 32) | d.nFileSizeLow) == ShrimpManifest.size)) {
			uprintf("Shrimp: obraz już pobrany, sprawdzanie sumy %s", ShrimpIsoPath);
			if (hash_file(ShrimpIsoPath, ShrimpManifest.size, digest) && hex_equal(digest, ShrimpManifest.sha256)) {
				ok = TRUE;
				goto out;
			}
			DeleteFileU(ShrimpIsoPath);
		}
	}

	for (int attempt = 0; ; attempt++) {
		BOOL last = attempt >= FETCH_ATTEMPTS - 1;
		uint64_t have = file_size_u(part);
		DWORD status = 0;
		if (have > ShrimpManifest.size) {
			DeleteFileU(part);
			have = 0;
		}
		if (attempt == 0 && free_space_u(dir, &free_bytes)
			&& free_bytes.QuadPart < ShrimpManifest.size - have + 256ULL * 1024 * 1024) {
			char m[160];
			static_sprintf(m, "Do pobrania Shrimp potrzeba %.1f GB wolnego miejsca na dysku %c:.",
				(ShrimpManifest.size - have + 256.0 * 1024 * 1024) / (1024.0 * 1024 * 1024), dir[0]);
			for (char* c = m; *c; c++)
				if (*c == '.' && c[1] >= '0' && c[1] <= '9')
					*c = ',';
			ShrimpFail(1, "space", m);
			goto out;
		}
		done = have;
		if (have < ShrimpManifest.size) {
			net_quiet = !last;
			net_kind = NULL;
			net_fatal = FALSE;
			r = open_url(session, ShrimpManifest.url, &len, ACCEPT_BINARY, have, &status);
			net_quiet = FALSE;
			if (r == NULL && net_fatal) {
				ShrimpFail(1, net_kind, net_msg[0] ? net_msg : NULL);
				goto out;
			}
			if (r != NULL) {
				if (have > 0 && status != 206) {
					uprintf("Shrimp: serwer nie wznawia pobierania (HTTP %lu), od początku", status);
					have = done = 0;
				} else if (have > 0) {
					uprintf("Shrimp: wznawiam pobieranie od %llu B", have);
				}
				f = CreateFileU(part, GENERIC_WRITE, 0, NULL, have ? OPEN_ALWAYS : CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
				if (f == INVALID_HANDLE_VALUE) {
					ShrimpFail(1, "other", "writeDl");
					goto out;
				}
				if (have) {
					LARGE_INTEGER pos;
					pos.QuadPart = (LONGLONG)have;
					SetFilePointerEx(f, pos, NULL, FILE_BEGIN);
					SetEndOfFile(f);
				}
				ShrimpSpeed(0, TRUE);
				for (;;) {
					if (ShrimpCancel)
						goto out;	/* część zostaje: następna próba ją dokończy */
					if (!InternetReadFile(r, buf, sizeof(buf), &n)) {
						DWORD e = GetLastError();
						net_kind = offline_error(e) ? "offline" : "other";
						uprintf("Shrimp: odczyt przerwany (błąd %lu) po %llu B", e, done);
						break;
					}
					if (n == 0)
						break;
					if (done + n > ShrimpManifest.size) {
						ShrimpFail(1, "verify", NULL);	/* serwer daje więcej, niż mówi manifest */
						CloseHandle(f);
						f = INVALID_HANDLE_VALUE;
						DeleteFileU(part);
						goto out;
					}
					if (!WriteFile(f, buf, n, &w, NULL) || w != n) {
						ShrimpFail(1, GetLastError() == ERROR_DISK_FULL ? "space" : "other", "writeDl");
						goto out;
					}
					done += n;
					ShrimpProgress(1, done, ShrimpManifest.size, ShrimpSpeed(done, FALSE));
				}
				CloseHandle(f);
				f = INVALID_HANDLE_VALUE;
				InternetCloseHandle(r);
				r = NULL;
			}
		}
		if (done == ShrimpManifest.size)
			break;
		/* Połączenie zerwane (WinInet kończy wtedy odczyt bez błędu albo z błędem sieci). */
		if (last) {
			ShrimpFail(1, net_kind != NULL ? net_kind : "offline", net_msg[0] ? net_msg : NULL);
			goto out;
		}
		uprintf("Shrimp: próba %d z %d nieudana, ponawiam", attempt + 1, FETCH_ATTEMPTS);
		for (DWORD t = 0; t < (2000UL << attempt) && !ShrimpCancel; t += 100)
			Sleep(100);
		if (ShrimpCancel)
			goto out;
	}

	/* Suma liczona z pliku na dysku: obejmuje też część pobraną przy wcześniejszym uruchomieniu. */
	if (!hash_file(part, ShrimpManifest.size, digest))
		goto out;
	if (!hex_equal(digest, ShrimpManifest.sha256)) {
		DeleteFileU(part);
		ShrimpFail(1, "verify", NULL);
		goto out;
	}
	if (!MoveFileExU(part, ShrimpIsoPath, MOVEFILE_REPLACE_EXISTING)) {
		ShrimpFail(1, "other", "writeDl");
		goto out;
	}
	ok = TRUE;

out:
	/* Niedokończona część zostaje na dysku: następne uruchomienie wznowi pobieranie od tego miejsca. */
	if (f != INVALID_HANDLE_VALUE)
		CloseHandle(f);
	if (r != NULL)
		InternetCloseHandle(r);
	if (session != NULL)
		InternetCloseHandle(session);
	free(manifest);
	free(sig);
	PostMessage(hShrimpWnd, WM_SHRIMP_FETCHED, ok && !ShrimpCancel, 0);
	return 0;
}
