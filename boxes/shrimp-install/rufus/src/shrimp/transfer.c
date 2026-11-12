/*
 * Shrimp Install: kopia plików użytkownika na nośnik i shrimp.json dla Setup.
 *
 * Na nośniku (FAT32):
 *   shrimp-transfer/shrimp.json          opis (schemat w shrimp-install/README.md)
 *   shrimp-transfer/files/<klucz>/…      Moje foldery: documents, pictures, desktop, music, videos, downloads
 *   shrimp-transfer/files/other/<nazwa>  Wybiorę sam
 *   shrimp-backup/…                      dawna zawartość nośnika (usbbackup.c), wraca na niego po instalacji
 * Pliki większe niż pozwala FAT32 dzielimy na części <plik>.shrimp-part000, 001…; Setup je skleja.
 *
 * Copyright © 2026 Shrimp. GPLv3.
 */
#include <windows.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>
#include "shrimp.h"
#include "../rufus.h"
#include "../msapi_utf8.h"
#include "../drive.h"

#define PART_SIZE (2ULL * 1024 * 1024 * 1024)
#define FAT32_MAX (4ULL * 1024 * 1024 * 1024 - 1)
#ifndef FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS
#define FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS 0x00400000
#endif
#ifndef FILE_ATTRIBUTE_RECALL_ON_OPEN
#define FILE_ATTRIBUTE_RECALL_ON_OPEN 0x00040000
#endif
#define CLOUD_ONLY (FILE_ATTRIBUTE_OFFLINE | FILE_ATTRIBUTE_RECALL_ON_DATA_ACCESS | FILE_ATTRIBUTE_RECALL_ON_OPEN)

shrimp_transfer ShrimpTransfer;

typedef struct {
	uint64_t done, total;
	jb split, skipped;
	const wchar_t* dest_root;   /* \\?\E:\shrimp-transfer */
	BOOL fatal;
} copy_ctx;

static wchar_t* join(const wchar_t* a, const wchar_t* b)
{
	size_t n = wcslen(a) + wcslen(b) + 2;
	wchar_t* r = malloc(n * sizeof(wchar_t));
	if (r != NULL)
		swprintf(r, n, L"%s\\%s", a, b);
	return r;
}

static wchar_t* long_path(const wchar_t* p)
{
	if (wcsncmp(p, L"\\\\", 2) == 0)
		return _wcsdup(p);
	size_t n = wcslen(p) + 5;
	wchar_t* r = malloc(n * sizeof(wchar_t));
	if (r != NULL)
		swprintf(r, n, L"\\\\?\\%s", p);
	return r;
}

/* Ścieżka względem shrimp-transfer z „/”, do shrimp.json. */
static void rel(copy_ctx* c, const wchar_t* dest, char* out, size_t size)
{
	const wchar_t* r = dest + wcslen(c->dest_root);
	while (*r == L'\\')
		r++;
	WideCharToMultiByte(CP_UTF8, 0, r, -1, out, (int)size, NULL, NULL);
	for (char* p = out; *p; p++)
		if (*p == '\\')
			*p = '/';
}

static void skip(copy_ctx* c, const wchar_t* src, const char* reason)
{
	jb_obj(&c->skipped, NULL);
	jb_wstr(&c->skipped, "source", wcsncmp(src, L"\\\\?\\", 4) == 0 ? src + 4 : src);
	jb_str(&c->skipped, "reason", reason);
	jb_end_obj(&c->skipped);
}

/* Błąd zapisu na nośnik przerywa całość; błąd odczytu jednego pliku tylko go pomija. */
static BOOL dest_error(copy_ctx* c, DWORD e)
{
	if (e == ERROR_DISK_FULL || e == ERROR_HANDLE_DISK_FULL) {
		ShrimpFail(3, "copy", "mediaFull");
	} else if (e == ERROR_NOT_READY || e == ERROR_DEVICE_NOT_CONNECTED || e == ERROR_DEV_NOT_EXIST
		|| e == ERROR_PATH_NOT_FOUND || e == ERROR_WRITE_FAULT || e == ERROR_GEN_FAILURE) {
		ShrimpFail(3, "device", NULL);
	} else {
		return FALSE;
	}
	c->fatal = TRUE;
	return TRUE;
}

static DWORD CALLBACK on_copy(LARGE_INTEGER total, LARGE_INTEGER done, LARGE_INTEGER s1, LARGE_INTEGER s2,
	DWORD stream, DWORD reason, HANDLE h1, HANDLE h2, LPVOID param)
{
	copy_ctx* c = param;
	(void)total; (void)s1; (void)s2; (void)stream; (void)reason; (void)h1; (void)h2;
	ShrimpProgress(3, c->done + done.QuadPart, c->total, ShrimpSpeed(c->done + done.QuadPart, FALSE));
	return ShrimpCancel ? PROGRESS_CANCEL : PROGRESS_CONTINUE;
}

static void copy_split(copy_ctx* c, const wchar_t* src, const wchar_t* dst, uint64_t size)
{
	static uint8_t buf[4 * 1024 * 1024];
	char r[MAX_PATH * 3];
	DWORD n, w;
	uint64_t written = 0;
	HANDLE in = CreateFileW(src, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, NULL), out = INVALID_HANDLE_VALUE;
	if (in == INVALID_HANDLE_VALUE) {
		skip(c, src, "read");
		c->done += size;
		return;
	}
	for (int part = 0; written < size && !ShrimpCancel && !c->fatal; part++) {
		size_t len = wcslen(dst) + 24;
		wchar_t* name = malloc(len * sizeof(wchar_t));
		uint64_t in_part = 0;
		if (name == NULL)
			break;
		swprintf(name, len, L"%s.shrimp-part%03d", dst, part);
		out = CreateFileW(name, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
		free(name);
		if (out == INVALID_HANDLE_VALUE) {
			dest_error(c, GetLastError());
			break;
		}
		while (in_part < PART_SIZE && !ShrimpCancel) {
			DWORD want = (DWORD)min(sizeof(buf), PART_SIZE - in_part);
			if (!ReadFile(in, buf, want, &n, NULL) || n == 0)
				break;
			if (!WriteFile(out, buf, n, &w, NULL) || w != n) {
				if (!dest_error(c, GetLastError()))
					ShrimpFail(3, "copy", NULL), c->fatal = TRUE;
				break;
			}
			in_part += n;
			written += n;
			ShrimpProgress(3, c->done + written, c->total, ShrimpSpeed(c->done + written, FALSE));
		}
		CloseHandle(out);
		if (in_part == 0)
			break;
	}
	CloseHandle(in);
	c->done += size;
	if (written == size) {
		rel(c, dst, r, sizeof(r));
		jb_str(&c->split, NULL, r);
	} else if (!c->fatal && !ShrimpCancel) {
		skip(c, src, "read");
	}
}

static void copy_file(copy_ctx* c, const wchar_t* src, const wchar_t* dst, uint64_t size)
{
	BOOL cancel = FALSE;
	if (size > FAT32_MAX) {
		copy_split(c, src, dst, size);
		return;
	}
	if (!CopyFileExW(src, dst, on_copy, c, &cancel, COPY_FILE_ALLOW_DECRYPTED_DESTINATION)) {
		DWORD e = GetLastError();
		if (e != ERROR_REQUEST_ABORTED && !dest_error(c, e)) {
			uprintf("Shrimp: pominięto plik (%lu)", e);
			skip(c, src, e == ERROR_SHARING_VIOLATION || e == ERROR_LOCK_VIOLATION ? "locked" : "read");
		}
	}
	c->done += size;
}

static void copy_tree(copy_ctx* c, const wchar_t* src, const wchar_t* dst)
{
	WIN32_FIND_DATAW fd;
	wchar_t* pat = join(src, L"*");
	HANDLE h;
	if (pat == NULL)
		return;
	if (!CreateDirectoryW(dst, NULL) && GetLastError() != ERROR_ALREADY_EXISTS) {
		dest_error(c, GetLastError());
		free(pat);
		return;
	}
	h = FindFirstFileExW(pat, FindExInfoBasic, &fd, FindExSearchNameMatch, NULL, FIND_FIRST_EX_LARGE_FETCH);
	free(pat);
	if (h == INVALID_HANDLE_VALUE)
		return;
	do {
		if (ShrimpCancel || c->fatal)
			break;
		if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0)
			continue;
		if (_wcsicmp(fd.cFileName, L"desktop.ini") == 0 || _wcsicmp(fd.cFileName, L"thumbs.db") == 0)
			continue;
		wchar_t* s = join(src, fd.cFileName);
		wchar_t* d = join(dst, fd.cFileName);
		if (s != NULL && d != NULL) {
			if (fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT) {
				/* dowiązania nie są plikami użytkownika */
			} else if (fd.dwFileAttributes & CLOUD_ONLY) {
				skip(c, s, "cloud");
			} else if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
				copy_tree(c, s, d);
			} else {
				copy_file(c, s, d, ((uint64_t)fd.nFileSizeHigh << 32) | fd.nFileSizeLow);
			}
		}
		free(s);
		free(d);
	} while (FindNextFileW(h, &fd));
	FindClose(h);
}


/* Znacznik języka Shrimp (boxes/<box>/i18n/<znacznik>.txt, też GRUB: boxes/grub-theme/i18n, .../themes/shrimp/lang/<znacznik>.cfg) z nazwy języka Windows (BCP-47). */
void ShrimpLangTag(const char* bcp, char* out, size_t n)
{
	char b[64], *p, lang[16] = "";
	size_t i;

	for (i = 0; bcp[i] && i < sizeof(b) - 1; i++)
		b[i] = (bcp[i] >= 'A' && bcp[i] <= 'Z') ? (char)(bcp[i] + 32) : bcp[i];
	b[i] = 0;
	for (i = 0; b[i] && b[i] != '-' && i < sizeof(lang) - 1; i++)
		lang[i] = b[i];
	lang[i] = 0;
	p = b + i; /* reszta: -hant-tw, -rs, -es-valencia… */
	if (strcmp(lang, "zh") == 0)
		strcpy(lang, (strstr(p, "-hant") || strstr(p, "-tw") || strstr(p, "-hk") || strstr(p, "-mo")) ? "zh-hant" : "zh-hans");
	else if (strcmp(lang, "pt") == 0)
		strcpy(lang, strstr(p, "-pt") ? "pt-pt" : "pt-br");
	else if (strcmp(lang, "sr") == 0)
		strcpy(lang, strstr(p, "-latn") ? "sr-latn" : "sr-cyrl");
	else if (strcmp(lang, "uz") == 0 && strstr(p, "-cyrl"))
		strcpy(lang, "uz-cyrl");
	else if (strcmp(lang, "be") == 0 && strstr(p, "-latn"))
		strcpy(lang, "be-latn");
	else if (strcmp(lang, "ca") == 0 && strstr(p, "valencia"))
		strcpy(lang, "ca-valencia");
	else if (strcmp(lang, "no") == 0)
		strcpy(lang, "nb");
	snprintf(out, n, "%s", lang[0] ? lang : "en");
}

/* boot/grub2/shrimp-lang.cfg na nośniku: GRUB (boxes/grub-theme/edit_boot_config.sh) czyta z niego język menu.
 * Brak pliku albo język bez tłumaczenia to angielski. Błąd zapisu nie przerywa przygotowania nośnika. */
static void write_grub_lang(const char* root)
{
	char path[MAX_PATH], tag[32] = "en", cfg[64], u[64];
	ULONG n = 0, len = 0;
	DWORD w;
	HANDLE h;

	if (GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, NULL, &len) && len > 1) {
		wchar_t* buf = calloc(len, sizeof(wchar_t));
		if (buf != NULL && GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, buf, &len) &&
			WideCharToMultiByte(CP_UTF8, 0, buf, -1, u, sizeof(u), NULL, NULL) > 0)
			ShrimpLangTag(u, tag, sizeof(tag));
		free(buf);
	}
	static_sprintf(path, "%sboot\\grub2\\shrimp-lang.cfg", root);
	h = CreateFileU(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
	if (h == INVALID_HANDLE_VALUE)
		return;
	snprintf(cfg, sizeof(cfg), "set shrimp_lang=%s\n", tag);
	WriteFile(h, cfg, (DWORD)strlen(cfg), &w, NULL);
	CloseHandle(h);
}

static BOOL write_json(const char* root, copy_ctx* c, jb* files)
{
	char path[MAX_PATH], now[32];
	SYSTEMTIME st;
	jb b;
	DWORD w;
	HANDLE h;
	BOOL ok;

	GetSystemTime(&st);
	static_sprintf(now, "%04d-%02d-%02dT%02d:%02d:%02dZ", st.wYear, st.wMonth, st.wDay, st.wHour, st.wMinute, st.wSecond);
	jb_init(&b);
	jb_obj(&b, NULL);
	jb_num(&b, "version", 1);
	jb_str(&b, "created", now);
	jb_str(&b, "installer", "Shrimp Install " SHRIMP_VERSION);
	jb_obj(&b, "image");
	jb_str(&b, "version", ShrimpManifest.version[0] ? ShrimpManifest.version : NULL);
	jb_str(&b, "sha256", ShrimpManifest.sha256[0] ? ShrimpManifest.sha256 : NULL);
	jb_end_obj(&b);
	ShrimpCollect(&b);
	jb_raw(&b, "files", files->s);
	jb_raw(&b, "split", c->split.s);
	jb_raw(&b, "skipped", c->skipped.s);
	jb_end_obj(&b);

	static_sprintf(path, "%s" SHRIMP_TRANSFER_DIR "\\shrimp.json", root);
	h = CreateFileU(path, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
	ok = h != INVALID_HANDLE_VALUE && WriteFile(h, b.s, (DWORD)b.len, &w, NULL) && w == b.len;
	if (h != INVALID_HANDLE_VALUE)
		CloseHandle(h);
	jb_free(&b);
	write_grub_lang(root);
	return ok;
}

DWORD WINAPI ShrimpCopyThread(LPVOID param)
{
	shrimp_transfer* t = &ShrimpTransfer;
	copy_ctx c;
	wchar_t root_w[16], *dest_root = NULL;
	char root[8];
	jb files;
	BOOL ok = FALSE;

	(void)param;
	memset(&c, 0, sizeof(c));
	jb_init(&c.split);
	jb_init(&c.skipped);
	jb_init(&files);
	jb_arr(&c.split, NULL);
	jb_arr(&c.skipped, NULL);
	jb_arr(&files, NULL);

	/* Po formatowaniu Windows potrzebuje chwili, by nadać nośnikowi literę. */
	{
		char letters[27] = "";
		for (int i = 0; i < 40 && !ShrimpCancel; i++) {
			if (GetDriveLetters(t->DriveIndex, letters) && letters[0])
				break;
			Sleep(500);
		}
		if (!letters[0]) {
			ShrimpFail(3, "device", NULL);
			goto out;
		}
		static_sprintf(root, "%c:\\", letters[0]);
	}
	MultiByteToWideChar(CP_UTF8, 0, root, -1, root_w, ARRAYSIZE(root_w));
	size_t n = wcslen(root_w) + 32;
	dest_root = malloc(n * sizeof(wchar_t));
	if (dest_root == NULL)
		goto out;
	swprintf(dest_root, n, L"\\\\?\\%s" SHRIMP_TRANSFER_DIR, root_w);
	c.dest_root = dest_root;
	if (!CreateDirectoryW(dest_root, NULL) && GetLastError() != ERROR_ALREADY_EXISTS) {
		ShrimpFail(3, "device", NULL);
		goto out;
	}
	wchar_t* files_dir = join(dest_root, L"files");
	if (files_dir == NULL)
		goto out;
	CreateDirectoryW(files_dir, NULL);

	for (int i = 0; i < t->count; i++)
		c.total += ShrimpTreeSize(t->items[i].source, &ShrimpCancel);
	c.total += 2 * ShrimpUsbBackupBytes;    /* zapis i sprawdzenie zawartości nośnika */
	ShrimpSpeed(0, TRUE);
	ShrimpProgress(3, 0, c.total, 0);

	/* Najpierw dawna zawartość nośnika: to jedyna kopia tych plików. */
	{
		wchar_t media[16];
		swprintf(media, ARRAYSIZE(media), L"\\\\?\\%c:", root[0]);
		if (!ShrimpUsbWriteBack(media, &c.done, c.total)) {
			c.fatal = TRUE;
			free(files_dir);
			goto out;
		}
	}

	for (int i = 0; i < t->count && !ShrimpCancel && !c.fatal; i++) {
		shrimp_item* it = &t->items[i];
		wchar_t wdest[MAX_PATH];
		char r[MAX_PATH * 3];
		uint64_t before = c.done;
		MultiByteToWideChar(CP_UTF8, 0, it->dest, -1, wdest, ARRAYSIZE(wdest));
		for (wchar_t* p = wdest; *p; p++)
			if (*p == L'/')
				*p = L'\\';
		wchar_t* src = long_path(it->source);
		wchar_t* dst = join(dest_root, wdest);
		if (src == NULL || dst == NULL) {
			free(src); free(dst);
			continue;
		}
		if (strncmp(it->dest, "files/other", 11) == 0) {
			wchar_t* other = join(files_dir, L"other");
			if (other != NULL)
				CreateDirectoryW(other, NULL);
			free(other);
		}
		DWORD a = GetFileAttributesW(src);
		if (a == INVALID_FILE_ATTRIBUTES)
			skip(&c, src, "missing");
		else if (a & FILE_ATTRIBUTE_DIRECTORY)
			copy_tree(&c, src, dst);
		else
			copy_file(&c, src, dst, ShrimpTreeSize(src, NULL));
		rel(&c, dst, r, sizeof(r));
		jb_obj(&files, NULL);
		jb_str(&files, "key", it->key);
		jb_str(&files, "path", r);
		jb_wstr(&files, "source", it->source);
		jb_u64(&files, "bytes", c.done - before);
		jb_end_obj(&files);
		free(src);
		free(dst);
	}
	free(files_dir);
	if (ShrimpCancel || c.fatal)
		goto out;
	jb_end_arr(&c.split);
	jb_end_arr(&c.skipped);
	jb_end_arr(&files);
	if (!write_json(root, &c, &files)) {
		ShrimpFail(3, "copy", "descWrite");
		goto out;
	}
	ShrimpProgress(3, c.total, c.total, 0);
	ok = TRUE;

out:
	free(dest_root);
	jb_free(&c.split);
	jb_free(&c.skipped);
	jb_free(&files);
	PostMessage(hShrimpWnd, WM_SHRIMP_COPIED, ok && !ShrimpCancel, 0);
	return 0;
}
