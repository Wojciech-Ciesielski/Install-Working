/*
 * Shrimp Install: zawartość nośnika nie ginie przy jego przygotowaniu.
 *
 *   1. Przed formatowaniem (ShrimpUsbBackupThread): wszystkie pliki z nośnika trafiają do
 *      %LOCALAPPDATA%\Shrimp\usb-backup\<id nośnika>\files, z SHA-256 liczonym przy odczycie.
 *      Kopia jest potem czytana z dysku jeszcze raz (bez pamięci podręcznej) i porównywana.
 *      Dopiero zgodna kopia pozwala nacisnąć START w Rufusie. Każdy błąd kończy przygotowanie,
 *      zanim cokolwiek zostanie skasowane.
 *   2. Po zapisie Shrimp (ShrimpUsbWriteBack, w fazie kopii): pliki idą do osobnego folderu
 *      shrimp-backup/ na gotowym nośniku, znowu z weryfikacją odczytem. Opis manifest.tsv jest
 *      zapisywany na końcu: bez niego Setup nośnika nie rusza. Kopia w Windows znika dopiero
 *      po udanej weryfikacji na nośniku.
 *   3. Setup (setup/shell/src/usb.rs) po instalacji przenosi kopię na dysk komputera, a przy
 *      pierwszym uruchomieniu formatuje nośnik i przywraca na nim pliki (opis w shrimp-install/README.md).
 *
 * manifest.tsv (UTF-8, ścieżki z „/”, bez tabulatorów w nazwach, bo Windows ich nie dopuszcza):
 *   #shrimp-backup 1
 *   label<TAB>etykieta          fs<TAB>FAT32|exFAT|NTFS|      scheme<TAB>mbr|gpt
 *   d<TAB>ścieżka               (folder, także pusty)
 *   f<TAB>sha256<TAB>bajty<TAB>mtime_unix<TAB>części<TAB>ścieżka
 * części = 0: jeden plik; N > 0: <ścieżka>.shrimp-part000 … (FAT32 nie mieści plików ≥ 4 GB).
 *
 * Copyright © 2026 Shrimp. GPLv3.
 */
#include <windows.h>
#include <shlobj.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>
#include "shrimp.h"
#include "../rufus.h"
#include "../msapi_utf8.h"
#include "../drive.h"

#define BUF_SIZE (4 * 1024 * 1024)
#define PART_SIZE (2ULL * 1024 * 1024 * 1024)
#define FAT32_MAX (4ULL * 1024 * 1024 * 1024 - 1)
#define MARGIN (512ULL * 1024 * 1024)
#define LIVE_LABEL L"SHRIMP_LIVE"

typedef struct {
	char* rel;          /* UTF-8, „/” */
	uint64_t size;
	int64_t mtime;
	uint8_t sha[32];
	int parts;
	BOOL dir;
} bk_entry;

typedef struct {
	bk_entry* e;
	int n, cap;
	uint64_t bytes;
	char label[64], fs[16], scheme[8];
} bk_list;

uint64_t ShrimpUsbBackupBytes = 0;
static wchar_t staging[MAX_PATH];   /* \\?\C:\Users\…\Shrimp\usb-backup\<id> */
static bk_list current;
static uint8_t* buf;

extern RUFUS_DRIVE rufus_drive[MAX_DRIVES];
extern RUFUS_DRIVE_INFO SelectedDrive;

/* ---------- drobiazgi ---------- */
static wchar_t* wjoin(const wchar_t* a, const wchar_t* b)
{
	size_t n = wcslen(a) + wcslen(b) + 2;
	wchar_t* r = malloc(n * sizeof(wchar_t));
	if (r != NULL)
		swprintf(r, n, L"%s\\%s", a, b);
	return r;
}

/* „DCIM/a.jpg” → L"DCIM\\a.jpg" */
static wchar_t* rel_w(const char* rel)
{
	wchar_t* w = utf8_to_wchar(rel);
	for (wchar_t* p = w; p != NULL && *p; p++)
		if (*p == L'/')
			*p = L'\\';
	return w;
}

static void hex(const uint8_t* d, char* out)
{
	for (int i = 0; i < 32; i++)
		sprintf(&out[i * 2], "%02x", d[i]);
}

static BOOL unhex(const char* s, uint8_t* d)
{
	if (strlen(s) != 64)
		return FALSE;
	for (int i = 0; i < 32; i++) {
		unsigned v;
		if (sscanf(&s[i * 2], "%2x", &v) != 1)
			return FALSE;
		d[i] = (uint8_t)v;
	}
	return TRUE;
}

static int64_t unix_time(const FILETIME* ft)
{
	uint64_t t = ((uint64_t)ft->dwHighDateTime << 32) | ft->dwLowDateTime;
	return (int64_t)(t / 10000000ULL) - 11644473600LL;
}

static FILETIME file_time(int64_t u)
{
	FILETIME ft;
	uint64_t t = (uint64_t)(u + 11644473600LL) * 10000000ULL;
	ft.dwLowDateTime = (DWORD)t;
	ft.dwHighDateTime = (DWORD)(t >> 32);
	return ft;
}

static void list_free(bk_list* l)
{
	for (int i = 0; i < l->n; i++)
		free(l->e[i].rel);
	free(l->e);
	memset(l, 0, sizeof(*l));
}

static bk_entry* list_add(bk_list* l, const char* rel, BOOL dir)
{
	if (l->n == l->cap) {
		int cap = l->cap ? l->cap * 2 : 256;
		bk_entry* e = realloc(l->e, cap * sizeof(bk_entry));
		if (e == NULL)
			return NULL;
		l->e = e;
		l->cap = cap;
	}
	bk_entry* e = &l->e[l->n++];
	memset(e, 0, sizeof(*e));
	e->rel = _strdup(rel);
	e->dir = dir;
	return e->rel != NULL ? e : (l->n--, NULL);
}

/* Usuwa folder z zawartością (kopia w Windows po udanym zapisie na nośnik). */
static void rm_tree(const wchar_t* dir)
{
	WIN32_FIND_DATAW fd;
	wchar_t* pat = wjoin(dir, L"*");
	HANDLE h = pat ? FindFirstFileW(pat, &fd) : INVALID_HANDLE_VALUE;
	free(pat);
	if (h != INVALID_HANDLE_VALUE) {
		do {
			if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0)
				continue;
			wchar_t* p = wjoin(dir, fd.cFileName);
			if (p == NULL)
				continue;
			if ((fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) && !(fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT)) {
				rm_tree(p);
			} else {
				SetFileAttributesW(p, FILE_ATTRIBUTE_NORMAL);
				DeleteFileW(p);
			}
			free(p);
		} while (FindNextFileW(h, &fd));
		FindClose(h);
	}
	RemoveDirectoryW(dir);
}

/* Tworzy wszystkie foldery nad plikiem (ścieżka z \\?\). */
static BOOL make_parents(const wchar_t* path)
{
	wchar_t* p = _wcsdup(path);
	BOOL ok = TRUE;
	if (p == NULL)
		return FALSE;
	for (wchar_t* s = p + 7; *s; s++) {     /* za „\\?\C:\” */
		if (*s != L'\\')
			continue;
		*s = 0;
		if (!CreateDirectoryW(p, NULL) && GetLastError() != ERROR_ALREADY_EXISTS)
			ok = FALSE;
		*s = L'\\';
	}
	free(p);
	return ok;
}

static void progress(int phase, uint64_t* done, uint64_t total, uint64_t add)
{
	*done += add;
	ShrimpProgressStep(phase, phase == 2 ? "backup" : NULL, *done, total, ShrimpSpeed(*done, FALSE));
}

/* ---------- odczyt z sumą ---------- */

/* Czyta plik (albo jego części) w całości, licząc SHA-256. nocache: odczyt z nośnika, nie z pamięci.
 * out != INVALID_HANDLE_VALUE: jednocześnie zapisuje (kopia z sumą liczona z danych źródła). */
static BOOL hash_read(const wchar_t* path, int parts, BOOL nocache, HANDLE out, uint64_t* size, uint8_t* digest,
	int phase, uint64_t* done, uint64_t total)
{
	HASH_CONTEXT ctx;
	uint64_t got = 0;
	BOOL ok = TRUE;
	hash_init[HASH_SHA256](&ctx);
	for (int part = 0; ok && part < (parts > 0 ? parts : 1); part++) {
		wchar_t* name = (wchar_t*)path;
		HANDLE in;
		DWORD n, w;
		if (parts > 0) {
			size_t len = wcslen(path) + 24;
			name = malloc(len * sizeof(wchar_t));
			if (name == NULL)
				return FALSE;
			swprintf(name, len, L"%s.shrimp-part%03d", path, part);
		}
		in = CreateFileW(name, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING,
			FILE_FLAG_SEQUENTIAL_SCAN | (nocache ? FILE_FLAG_NO_BUFFERING : 0), NULL);
		if (name != path)
			free(name);
		if (in == INVALID_HANDLE_VALUE)
			return FALSE;
		for (;;) {
			if (ShrimpCancel) {
				ok = FALSE;
				break;
			}
			if (!ReadFile(in, buf, BUF_SIZE, &n, NULL)) {
				ok = FALSE;
				break;
			}
			if (n == 0)
				break;
			hash_write[HASH_SHA256](&ctx, buf, n);
			if (out != INVALID_HANDLE_VALUE && (!WriteFile(out, buf, n, &w, NULL) || w != n)) {
				ok = FALSE;
				break;
			}
			got += n;
			if (done != NULL)
				progress(phase, done, total, n);
		}
		CloseHandle(in);
	}
	hash_final[HASH_SHA256](&ctx);
	memcpy(digest, ctx.buf, 32);
	if (size != NULL)
		*size = got;
	return ok;
}

/* ---------- manifest ---------- */
static BOOL write_manifest(const wchar_t* path, const bk_list* l)
{
	jb b;
	char line[MAX_PATH * 4 + 200], h[65];
	HANDLE f;
	DWORD w;
	BOOL ok;
	wchar_t* tmp;
	size_t n = wcslen(path) + 8;

	jb_init(&b);
	jb_text(&b, "#shrimp-backup 1\n");
	snprintf(line, sizeof(line), "label\t%s\nfs\t%s\nscheme\t%s\n", l->label, l->fs, l->scheme[0] ? l->scheme : "mbr");
	jb_text(&b, line);
	for (int i = 0; i < l->n; i++) {
		const bk_entry* e = &l->e[i];
		if (e->dir) {
			snprintf(line, sizeof(line), "d\t%s\n", e->rel);
		} else {
			hex(e->sha, h);
			snprintf(line, sizeof(line), "f\t%s\t%llu\t%lld\t%d\t%s\n", h, (unsigned long long)e->size,
				(long long)e->mtime, e->parts, e->rel);
		}
		jb_text(&b, line);
	}
	tmp = malloc(n * sizeof(wchar_t));
	if (tmp == NULL) {
		jb_free(&b);
		return FALSE;
	}
	swprintf(tmp, n, L"%s.tmp", path);
	f = CreateFileW(tmp, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
	ok = b.s != NULL && f != INVALID_HANDLE_VALUE && WriteFile(f, b.s, (DWORD)b.len, &w, NULL) && w == b.len && FlushFileBuffers(f);
	if (f != INVALID_HANDLE_VALUE)
		CloseHandle(f);
	ok = ok && MoveFileExW(tmp, path, MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH);
	if (!ok)
		DeleteFileW(tmp);
	free(tmp);
	jb_free(&b);
	return ok;
}

static BOOL read_manifest(const wchar_t* path, bk_list* l)
{
	HANDLE f = CreateFileW(path, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, 0, NULL);
	LARGE_INTEGER sz;
	char* s, *line, *ctx = NULL;
	DWORD r;
	BOOL ok = FALSE;

	memset(l, 0, sizeof(*l));
	if (f == INVALID_HANDLE_VALUE)
		return FALSE;
	if (!GetFileSizeEx(f, &sz) || sz.QuadPart > 256 * 1024 * 1024 || (s = malloc((size_t)sz.QuadPart + 1)) == NULL) {
		CloseHandle(f);
		return FALSE;
	}
	if (!ReadFile(f, s, (DWORD)sz.QuadPart, &r, NULL) || r != sz.QuadPart)
		goto out;
	s[r] = 0;
	if (strncmp(s, "#shrimp-backup 1\n", 17) != 0)
		goto out;
	for (line = strtok_s(s, "\n", &ctx); line != NULL; line = strtok_s(NULL, "\n", &ctx)) {
		char* f_[6] = { 0 };
		int nf = 0;
		for (char* p = line; nf < 6; ) {
			f_[nf++] = p;
			if (nf == 6)
				break;   /* ścieżka może zawierać wszystko poza tabulatorem */
			p = strchr(p, '\t');
			if (p == NULL)
				break;
			*p++ = 0;
		}
		if (strcmp(f_[0], "label") == 0 && nf == 2)
			static_strcpy(l->label, f_[1]);
		else if (strcmp(f_[0], "fs") == 0 && nf == 2)
			static_strcpy(l->fs, f_[1]);
		else if (strcmp(f_[0], "scheme") == 0 && nf == 2)
			static_strcpy(l->scheme, f_[1]);
		else if (strcmp(f_[0], "d") == 0 && nf == 2) {
			if (list_add(l, f_[1], TRUE) == NULL)
				goto out;
		} else if (strcmp(f_[0], "f") == 0 && nf == 6) {
			bk_entry* e = list_add(l, f_[5], FALSE);
			if (e == NULL || !unhex(f_[1], e->sha))
				goto out;
			e->size = _strtoui64(f_[2], NULL, 10);
			e->mtime = _strtoi64(f_[3], NULL, 10);
			e->parts = atoi(f_[4]);
			l->bytes += e->size;
		} else if (line[0] != '#') {
			goto out;
		}
	}
	ok = TRUE;
out:
	free(s);
	CloseHandle(f);
	if (!ok)
		list_free(l);
	return ok;
}

/* ---------- 1. kopia przed formatowaniem ---------- */

static BOOL skip_name(const wchar_t* n)
{
	return _wcsicmp(n, L"System Volume Information") == 0 || _wcsicmp(n, L"$RECYCLE.BIN") == 0 ||
		_wcsicmp(n, L"RECYCLER") == 0;
}

/* Zbiera listę plików i folderów z drzewa nośnika (bez kopiowania). */
static BOOL scan(const wchar_t* root, const wchar_t* sub, const char* prefix, bk_list* l)
{
	WIN32_FIND_DATAW fd;
	wchar_t* dir = sub[0] ? wjoin(root, sub) : _wcsdup(root);
	wchar_t* pat = dir ? wjoin(dir, L"*") : NULL;
	HANDLE h = pat ? FindFirstFileExW(pat, FindExInfoBasic, &fd, FindExSearchNameMatch, NULL, FIND_FIRST_EX_LARGE_FETCH)
		: INVALID_HANDLE_VALUE;
	BOOL ok = TRUE;
	free(pat);
	free(dir);
	if (h == INVALID_HANDLE_VALUE)
		return GetLastError() == ERROR_FILE_NOT_FOUND;
	do {
		if (ShrimpCancel)
			ok = FALSE;
		if (!ok)
			break;
		if (wcscmp(fd.cFileName, L".") == 0 || wcscmp(fd.cFileName, L"..") == 0)
			continue;
		if (!sub[0] && skip_name(fd.cFileName))
			continue;
		if (fd.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT)
			continue;
		wchar_t* s = sub[0] ? wjoin(sub, fd.cFileName) : _wcsdup(fd.cFileName);
		char* u = s ? wchar_to_utf8(s) : NULL;
		char rel[MAX_PATH * 4];
		if (u == NULL) {
			free(s);
			ok = FALSE;
			break;
		}
		snprintf(rel, sizeof(rel), "%s%s", prefix, u);
		for (char* p = rel; *p; p++)
			if (*p == '\\')
				*p = '/';
		if (fd.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY) {
			ok = list_add(l, rel, TRUE) != NULL && scan(root, s, prefix, l);
		} else {
			bk_entry* e = list_add(l, rel, FALSE);
			if (e == NULL) {
				ok = FALSE;
			} else {
				e->size = ((uint64_t)fd.nFileSizeHigh << 32) | fd.nFileSizeLow;
				e->mtime = unix_time(&fd.ftLastWriteTime);
				l->bytes += e->size;
			}
		}
		free(u);
		free(s);
	} while (FindNextFileW(h, &fd));
	FindClose(h);
	return ok;
}

/* Źródło jednego wpisu: zwykły plik na nośniku albo (nośnik Shrimp) plik z jego shrimp-backup. */
typedef struct {
	wchar_t root[MAX_PATH];     /* \\?\E:\ albo \\?\E:\shrimp-backup\files */
	char prefix[16];            /* „” albo „Partycja 2/” */
	BOOL from_backup;
} bk_source;

static const bk_source* source_of(const bk_source* src, int nsrc, const char* rel, const char** inner)
{
	for (int i = nsrc - 1; i >= 0; i--) {
		size_t k = strlen(src[i].prefix);
		if (strncmp(rel, src[i].prefix, k) == 0) {
			*inner = rel + k;
			return &src[i];
		}
	}
	return NULL;
}

/* Kopiuje listę ze źródeł do kopii w Windows i sprawdza ją. FALSE: błąd zgłoszony (ShrimpFail). */
static BOOL copy_to_staging(const bk_source* src, int nsrc, bk_list* l, BOOL check_known)
{
	wchar_t* files = wjoin(staging, L"files");
	uint64_t done = 0, total = l->bytes * 2;
	BOOL ok = files != NULL && (CreateDirectoryW(files, NULL) || GetLastError() == ERROR_ALREADY_EXISTS);

	ShrimpSpeed(0, TRUE);
	for (int i = 0; ok && i < l->n; i++) {
		bk_entry* e = &l->e[i];
		const char* inner = e->rel;
		const bk_source* s = source_of(src, nsrc, e->rel, &inner);
		wchar_t *rw = rel_w(e->rel), *iw = rel_w(inner);
		wchar_t *dst = rw ? wjoin(files, rw) : NULL, *from = (s && iw) ? wjoin(s->root, iw) : NULL;
		free(rw);
		free(iw);
		if (dst == NULL || from == NULL || !make_parents(dst)) {
			ok = FALSE;
		} else if (e->dir) {
			ok = CreateDirectoryW(dst, NULL) || GetLastError() == ERROR_ALREADY_EXISTS;
		} else {
			HANDLE out = CreateFileW(dst, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
			uint8_t d[32];
			uint64_t got = 0;
			if (out == INVALID_HANDLE_VALUE) {
				DWORD err = GetLastError();
				ShrimpFail(2, "space", err == ERROR_DISK_FULL ? "backupSpace" : "backupRead");
				free(dst); free(from); free(files);
				return FALSE;
			}
			ok = hash_read(from, s->from_backup ? e->parts : 0, FALSE, out, &got, d, 2, &done, total)
				&& got == e->size && FlushFileBuffers(out);
			if (ok) {
				FILETIME ft = file_time(e->mtime);
				SetFileTime(out, NULL, NULL, &ft);
			}
			CloseHandle(out);
			/* Nośnik Shrimp: suma z jego manifestu musi się zgadzać, zanim zaufamy tej kopii. */
			if (ok && check_known && s->from_backup && memcmp(d, e->sha, 32) != 0)
				ok = FALSE;
			memcpy(e->sha, d, 32);
			e->parts = 0;
		}
		free(dst);
		free(from);
	}
	/* Druga runda: kopia w Windows czytana z dysku i porównana z sumami ze źródła. */
	for (int i = 0; ok && i < l->n; i++) {
		bk_entry* e = &l->e[i];
		uint8_t d[32];
		uint64_t got = 0;
		if (e->dir)
			continue;
		wchar_t* rw = rel_w(e->rel);
		wchar_t* p = rw ? wjoin(files, rw) : NULL;
		ok = p != NULL && hash_read(p, 0, TRUE, INVALID_HANDLE_VALUE, &got, d, 2, &done, total)
			&& got == e->size && memcmp(d, e->sha, 32) == 0;
		free(rw);
		free(p);
	}
	free(files);
	if (!ok && !ShrimpCancel)
		ShrimpFail(2, "copy", "backupRead");
	return ok;
}

static BOOL find_staging(DWORD DriveIndex)
{
	PWSTR la = NULL;
	const char* id = NULL;
	HASH_CONTEXT ctx;
	char h[65];
	for (int i = 0; i < MAX_DRIVES && rufus_drive[i].size != 0; i++)
		if (rufus_drive[i].index == DriveIndex)
			id = rufus_drive[i].id;
	if (id == NULL || FAILED(SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, NULL, &la)))
		return FALSE;
	hash_init[HASH_SHA256](&ctx);
	hash_write[HASH_SHA256](&ctx, (const uint8_t*)id, strlen(id));
	hash_final[HASH_SHA256](&ctx);
	hex(ctx.buf, h);
	h[16] = 0;
	swprintf(staging, ARRAYSIZE(staging), L"\\\\?\\%s\\Shrimp", la);
	CreateDirectoryW(staging, NULL);
	wcscat_s(staging, ARRAYSIZE(staging), L"\\usb-backup");
	CreateDirectoryW(staging, NULL);
	wcscat_s(staging, ARRAYSIZE(staging), L"\\");
	MultiByteToWideChar(CP_UTF8, 0, h, -1, staging + wcslen(staging), (int)(ARRAYSIZE(staging) - wcslen(staging)));
	CoTaskMemFree(la);
	return TRUE;
}

static BOOL exists(const wchar_t* dir, const wchar_t* name)
{
	wchar_t* p = wjoin(dir, name);
	BOOL r = p != NULL && GetFileAttributesW(p) != INVALID_FILE_ATTRIBUTES;
	free(p);
	return r;
}

/* Główna część kopii: roots to katalogi główne woluminów nośnika (\\?\E:\ …). */
static BOOL backup_roots(wchar_t roots[][8], int nroots, uint64_t media_size, uint64_t shrimp_bytes)
{
	bk_source src[26];
	int nsrc = 0;
	ULARGE_INTEGER free_bytes;
	wchar_t* man;

	list_free(&current);
	rm_tree(staging);
	if (!CreateDirectoryW(staging, NULL)) {
		ShrimpFail(2, "space", "backupSpace");
		return FALSE;
	}
	for (int v = 0; v < nroots; v++) {
		wchar_t label[MAX_PATH + 1] = L"", fs[MAX_PATH + 1] = L"";
		bk_source* s = &src[nsrc];
		memset(s, 0, sizeof(*s));
		if (!GetVolumeInformationW(roots[v] + 4, label, ARRAYSIZE(label), NULL, NULL, NULL, fs, ARRAYSIZE(fs))) {
			/* Wolumin z literą, którego Windows nie umie przeczytać (np. zablokowany BitLocker To Go). */
			ShrimpFail(2, "copy", "backupRead");
			return FALSE;
		}
		if (nroots > 1)
			snprintf(s->prefix, sizeof(s->prefix), "%d/", v + 1);
		if (_wcsicmp(label, LIVE_LABEL) == 0) {
			/* Nośnik przygotowany już wcześniej: oryginalna zawartość jest w jego shrimp-backup. */
			wchar_t* b = wjoin(roots[v], L"shrimp-backup");
			wchar_t* m = b ? wjoin(b, L"manifest.tsv") : NULL;
			bk_list old;
			if (m != NULL && read_manifest(m, &old)) {
				swprintf(s->root, ARRAYSIZE(s->root), L"%s\\files", b);
				s->from_backup = TRUE;
				if (nsrc == 0) {
					static_strcpy(current.label, old.label);
					static_strcpy(current.fs, old.fs);
					static_strcpy(current.scheme, old.scheme);
				}
				for (int i = 0; i < old.n; i++) {
					char rel[MAX_PATH * 4];
					snprintf(rel, sizeof(rel), "%s%s", s->prefix, old.e[i].rel);
					bk_entry* e = list_add(&current, rel, old.e[i].dir);
					if (e == NULL)
						break;
					e->size = old.e[i].size;
					e->mtime = old.e[i].mtime;
					e->parts = old.e[i].parts;
					memcpy(e->sha, old.e[i].sha, 32);
					current.bytes += e->size;
				}
				list_free(&old);
				nsrc++;
			}
			/* Bez opisu: na nośniku jest tylko poprzedni Shrimp (obraz i pliki z Windows, które powstaną znowu). */
			free(m);
			free(b);
			continue;
		}
		if (nsrc == 0) {
			wchar_to_utf8_no_alloc(label, current.label, sizeof(current.label));
			wchar_to_utf8_no_alloc(fs, current.fs, sizeof(current.fs));
		}
		wcscpy_s(s->root, ARRAYSIZE(s->root), roots[v]);
		if (s->root[wcslen(s->root) - 1] == L'\\')
			s->root[wcslen(s->root) - 1] = 0;
		{
			wchar_t r[8];
			wcscpy_s(r, ARRAYSIZE(r), roots[v]);
			if (r[wcslen(r) - 1] == L'\\')
				r[wcslen(r) - 1] = 0;
			if (!scan(r, L"", s->prefix, &current)) {
				if (!ShrimpCancel)
					ShrimpFail(2, "copy", "backupRead");
				return FALSE;
			}
		}
		nsrc++;
	}
	static_strcpy(current.scheme, current.scheme[0] ? current.scheme : (SelectedDrive.PartitionStyle == PARTITION_STYLE_GPT ? "gpt" : "mbr"));

	/* Miejsce: kopia w Windows i potem na nośniku obok Shrimp. */
	if (current.bytes > 0) {
		if (!GetDiskFreeSpaceExW(staging, &free_bytes, NULL, NULL) || free_bytes.QuadPart < current.bytes + MARGIN) {
			ShrimpFail(2, "space", "backupSpace");
			return FALSE;
		}
		if (media_size != 0 && shrimp_bytes + current.bytes + current.bytes / 50 + (current.n * 32768ULL) + MARGIN > media_size) {
			ShrimpFail(2, "write", "backupMedia");
			return FALSE;
		}
	}
	if (!copy_to_staging(src, nsrc, &current, TRUE))
		return FALSE;
	man = wjoin(staging, L"manifest.tsv");
	BOOL ok = man != NULL && write_manifest(man, &current);
	free(man);
	if (!ok)
		ShrimpFail(2, "space", "backupSpace");
	return ok;
}

DWORD WINAPI ShrimpUsbBackupThread(LPVOID param)
{
	DWORD DriveIndex = (DWORD)(uintptr_t)param;
	char letters[27] = "";
	wchar_t roots[26][8];
	int nroots = 0;
	uint64_t media = 0, shrimp_bytes;
	BOOL ok = FALSE;

	ShrimpUsbBackupBytes = 0;
	if (buf == NULL)
		buf = VirtualAlloc(NULL, BUF_SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
	if (buf == NULL || !find_staging(DriveIndex)) {
		ShrimpFail(2, "space", "backupSpace");
		goto out;
	}
	/* Poprzednia próba sformatowała już ten nośnik: oryginały są tylko w kopii, nie robimy jej od nowa. */
	if (exists(staging, L"formatted")) {
		wchar_t* m = wjoin(staging, L"manifest.tsv");
		list_free(&current);
		ok = m != NULL && read_manifest(m, &current);
		free(m);
		if (!ok)
			ShrimpFail(2, "copy", "backupRead");
		goto out;
	}
	for (int i = 0; i < MAX_DRIVES && rufus_drive[i].size != 0; i++)
		if (rufus_drive[i].index == DriveIndex)
			media = rufus_drive[i].size;
	shrimp_bytes = ShrimpManifest.size ? ShrimpManifest.size : SHRIMP_ISO_ESTIMATE;
	for (int i = 0; i < ShrimpTransfer.count; i++)
		shrimp_bytes += ShrimpTreeSize(ShrimpTransfer.items[i].source, &ShrimpCancel);
	if (GetDriveLetters(DriveIndex, letters))
		for (char* p = letters; *p && nroots < 26; p++)
			swprintf(roots[nroots++], 8, L"\\\\?\\%c:\\", *p);
	ok = backup_roots(roots, nroots, media, shrimp_bytes);
out:
	if (ok)
		ShrimpUsbBackupBytes = current.bytes;
	PostMessage(hShrimpWnd, WM_SHRIMP_BACKED_UP, ok && !ShrimpCancel, 0);
	return 0;
}

/* Tuż przed START: od teraz oryginały są tylko w kopii. */
void ShrimpUsbBackupMarkFormatted(void)
{
	wchar_t* p = wjoin(staging, L"formatted");
	HANDLE h = p ? CreateFileW(p, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL | FILE_FLAG_WRITE_THROUGH, NULL)
		: INVALID_HANDLE_VALUE;
	if (h != INVALID_HANDLE_VALUE)
		CloseHandle(h);
	free(p);
}

/* ---------- 2. kopia na gotowy nośnik ---------- */

static BOOL copy_split_out(const wchar_t* from, const wchar_t* dst, uint64_t size, int* parts, uint64_t* done, uint64_t total)
{
	HANDLE in = CreateFileW(from, GENERIC_READ, FILE_SHARE_READ, NULL, OPEN_EXISTING, FILE_FLAG_SEQUENTIAL_SCAN, NULL);
	uint64_t written = 0;
	BOOL ok = in != INVALID_HANDLE_VALUE;
	*parts = 0;
	for (int part = 0; ok && (written < size || part == 0); part++) {
		size_t len = wcslen(dst) + 24;
		wchar_t* name = malloc(len * sizeof(wchar_t));
		uint64_t in_part = 0;
		HANDLE out;
		DWORD n, w;
		if (name == NULL) {
			ok = FALSE;
			break;
		}
		if (size > FAT32_MAX)
			swprintf(name, len, L"%s.shrimp-part%03d", dst, part);
		else
			wcscpy_s(name, len, dst);
		out = CreateFileW(name, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
		free(name);
		if (out == INVALID_HANDLE_VALUE) {
			ok = FALSE;
			break;
		}
		while (ok && written < size && (size <= FAT32_MAX || in_part < PART_SIZE)) {
			DWORD want = (DWORD)min((uint64_t)BUF_SIZE, size <= FAT32_MAX ? size - written : PART_SIZE - in_part);
			if (ShrimpCancel || !ReadFile(in, buf, want, &n, NULL) || n == 0 || !WriteFile(out, buf, n, &w, NULL) || w != n) {
				ok = FALSE;
				break;
			}
			in_part += n;
			written += n;
			progress(3, done, total, n);
		}
		ok = FlushFileBuffers(out) && ok;
		CloseHandle(out);
		if (size > FAT32_MAX)
			(*parts)++;
		if (size == 0)
			break;
	}
	if (in != INVALID_HANDLE_VALUE)
		CloseHandle(in);
	return ok && written == size;
}

BOOL ShrimpUsbWriteBack(const wchar_t* media_root, uint64_t* done, uint64_t total)
{
	wchar_t* bdir = wjoin(media_root, L"shrimp-backup");
	wchar_t* fdir = bdir ? wjoin(bdir, L"files") : NULL;
	wchar_t* sfiles = wjoin(staging, L"files");
	wchar_t* man = NULL;
	BOOL ok = bdir != NULL && fdir != NULL && sfiles != NULL && buf != NULL && staging[0];

	if (ok && (CreateDirectoryW(bdir, NULL) || GetLastError() == ERROR_ALREADY_EXISTS))
		ok = CreateDirectoryW(fdir, NULL) || GetLastError() == ERROR_ALREADY_EXISTS;
	if (ok)
		SetFileAttributesW(bdir, FILE_ATTRIBUTE_HIDDEN);
	for (int i = 0; ok && i < current.n; i++) {
		bk_entry* e = &current.e[i];
		wchar_t* rw = rel_w(e->rel);
		wchar_t* dst = rw ? wjoin(fdir, rw) : NULL;
		wchar_t* from = rw ? wjoin(sfiles, rw) : NULL;
		ok = dst != NULL && from != NULL && make_parents(dst);
		if (ok && e->dir)
			ok = CreateDirectoryW(dst, NULL) || GetLastError() == ERROR_ALREADY_EXISTS;
		else if (ok)
			ok = copy_split_out(from, dst, e->size, &e->parts, done, total);
		free(rw);
		free(dst);
		free(from);
	}
	/* Odczyt z nośnika (bez pamięci podręcznej) i porównanie sum. */
	for (int i = 0; ok && i < current.n; i++) {
		bk_entry* e = &current.e[i];
		uint8_t d[32];
		uint64_t got = 0;
		if (e->dir)
			continue;
		wchar_t* rw = rel_w(e->rel);
		wchar_t* p = rw ? wjoin(fdir, rw) : NULL;
		ok = p != NULL && hash_read(p, e->parts, TRUE, INVALID_HANDLE_VALUE, &got, d, 3, done, total)
			&& got == e->size && memcmp(d, e->sha, 32) == 0;
		free(rw);
		free(p);
	}
	if (ok) {
		bk_list check;
		man = wjoin(bdir, L"manifest.tsv");
		ok = man != NULL && write_manifest(man, &current) && read_manifest(man, &check) && check.n == current.n;
		if (ok)
			list_free(&check);
	}
	if (ok) {
		uprintf("Shrimp: zawartość nośnika (%d pozycji, %llu B) jest w shrimp-backup", current.n, (unsigned long long)current.bytes);
		rm_tree(staging);
	} else if (!ShrimpCancel) {
		uprintf("Shrimp: kopia zawartości nośnika została w %ls", staging);
		ShrimpFail(3, "copy", "backupVerify");
	}
	free(man);
	free(sfiles);
	free(fdir);
	free(bdir);
	return ok;
}

/* ---------- test bez nośnika (CI): --backup-test ŹRÓDŁO CEL KOPIA ---------- */
/* ŹRÓDŁO: katalog z plikami („pendrive”), CEL: katalog „gotowego nośnika”, KOPIA: miejsce kopii w Windows.
 * Kończy proces: 0 gdy kopia, zapis i ponowna kopia z nośnika Shrimp (z jego shrimp-backup) się zgadzają. */
int ShrimpUsbBackupTest(const wchar_t* src, const wchar_t* dst, const wchar_t* work)
{
	wchar_t roots[1][8];
	uint64_t done = 0;
	bk_source s;
	int rc = 1;
	(void)roots;
	buf = VirtualAlloc(NULL, BUF_SIZE, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
	swprintf(staging, ARRAYSIZE(staging), L"\\\\?\\%s\\stage", work);
	rm_tree(staging);
	CreateDirectoryW(staging, NULL);
	memset(&s, 0, sizeof(s));
	swprintf(s.root, ARRAYSIZE(s.root), L"\\\\?\\%s", src);
	list_free(&current);
	static_strcpy(current.label, "TEST");
	static_strcpy(current.fs, "exFAT");
	static_strcpy(current.scheme, "mbr");
	if (buf == NULL || !scan(s.root, L"", "", &current) || !copy_to_staging(&s, 1, &current, FALSE)) {
		fprintf(stderr, "kopia do Windows nie powiodła się\n");
		return 1;
	}
	{
		wchar_t* m = wjoin(staging, L"manifest.tsv");
		write_manifest(m, &current);
		free(m);
	}
	{
		wchar_t d[MAX_PATH];
		swprintf(d, ARRAYSIZE(d), L"\\\\?\\%s", dst);
		CreateDirectoryW(d, NULL);
		if (!ShrimpUsbWriteBack(d, &done, current.bytes * 2)) {
			fprintf(stderr, "zapis na nośnik nie powiódł się\n");
			return 1;
		}
		if (GetFileAttributesW(staging) != INVALID_FILE_ATTRIBUTES) {
			fprintf(stderr, "kopia w Windows nie została usunięta\n");
			return 1;
		}
		/* Ponowne przygotowanie tego samego nośnika: oryginały z jego shrimp-backup, z weryfikacją sum. */
		bk_source b;
		bk_list old;
		wchar_t* m = wjoin(d, L"shrimp-backup\\manifest.tsv");
		memset(&b, 0, sizeof(b));
		swprintf(b.root, ARRAYSIZE(b.root), L"%s\\shrimp-backup\\files", d);
		b.from_backup = TRUE;
		if (m == NULL || !read_manifest(m, &old) || old.n != current.n) {
			fprintf(stderr, "manifest na nośniku jest niepoprawny\n");
			return 1;
		}
		free(m);
		CreateDirectoryW(staging, NULL);
		if (!copy_to_staging(&b, 1, &old, TRUE)) {
			fprintf(stderr, "ponowna kopia z shrimp-backup nie powiodła się\n");
			return 1;
		}
		printf("kopia nośnika: %d pozycji, %llu B, zgodna\n", old.n, (unsigned long long)old.bytes);
		list_free(&old);
		rc = 0;
	}
	return rc;
}
