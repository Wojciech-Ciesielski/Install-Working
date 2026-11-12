/*
 * Shrimp Install: przebieg przygotowania nośnika i most do interfejsu.
 *
 *   start → pobranie ISO (fetch.c) → wybór nośnika i analiza ISO w Rufusie
 *         → kopia zawartości nośnika w Windows ze sprawdzeniem sum (usbbackup.c) → START w Rufusie
 *         → zawartość nośnika do shrimp-backup/, kopia plików i shrimp.json (transfer.c) → gotowe
 *         → BootNext i restart (boot.c)
 *
 * Argumenty: --demo (dane z makiety), --source PLIK.iso (bez pobierania), --manifest URL,
 * --collect PLIK.json (tylko zebranie danych z Windows, bez okna), --rufus (zwykły Rufus),
 * --screen NAZWA i --theme light|dark (podgląd ekranów z makiety),
 * --backup-test ŹRÓDŁO CEL KOPIA (test kopii zawartości nośnika na katalogach, bez okna),
 * --auto-disk NUMER (test w CI: od razu cały przebieg na dysku o tym numerze, np. podłączonym VHD;
 *   bez folderów użytkownika, kod wyjścia 0 po „done”, 3 po „fail”).
 *
 * Dziennik: %LOCALAPPDATA%\Shrimp Install\shrimp-install.log (poprzedni: shrimp-install.old.log).
 *
 * Copyright © 2026 Shrimp. GPLv3.
 */
#define COBJMACROS
#include <windows.h>
#include <windowsx.h>
#include <shobjidl.h>
#include <shellapi.h>
#include <shlobj.h>
#include <stdio.h>
#include <string.h>
#include "shrimp.h"
#include "../rufus.h"
#include "../msapi_utf8.h"
#include "../drive.h"
#include "../resource.h"
#include "../localization.h"

extern DWORD WINAPI ImageScanThread(LPVOID param);
extern HANDLE format_thread;
extern BOOL enable_HDDs, no_confirmation_on_cancel;
extern RUFUS_DRIVE rufus_drive[MAX_DRIVES];
extern char ShrimpManifestOverride[1024];

BOOL shrimp_mode = TRUE;
BOOL ShrimpBitLockerOn = FALSE;
static BOOL opt_demo = FALSE;
static char opt_collect[MAX_PATH], opt_screen[32], opt_theme[16], opt_lang[64];

static enum { ST_IDLE, ST_FETCH, ST_SCAN, ST_BACKUP, ST_FORMAT, ST_COPY, ST_DONE, ST_FAILED } state = ST_IDLE;
static DWORD plan_device;
static int cancel_id = 0;
static volatile LONG fail_sent = 0;
static char last_error[512];
/* Pytanie Rufusa, na które Shrimp nie odpowiada „tak” (rodzaj błędu dla interfejsu i tekst Rufusa). */
static const char* refused_kind;
static char refused_text[512];
static BOOL folders_started = FALSE;
static int opt_auto_disk = -1;
static volatile uint64_t copied_bytes = 0;
static BOOL speed_armed = FALSE;

/* ---------- dziennik w pliku ---------- */
static HANDLE log_file = INVALID_HANDLE_VALUE;
static CRITICAL_SECTION log_cs;

static void log_open(void)
{
	PWSTR la = NULL;
	wchar_t dir[MAX_PATH], path[MAX_PATH], old[MAX_PATH];
	if (FAILED(SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, NULL, &la)))
		return;
	swprintf(dir, ARRAYSIZE(dir), L"%s\\Shrimp Install", la);
	CoTaskMemFree(la);
	CreateDirectoryW(dir, NULL);
	swprintf(path, ARRAYSIZE(path), L"%s\\shrimp-install.log", dir);
	swprintf(old, ARRAYSIZE(old), L"%s\\shrimp-install.old.log", dir);
	MoveFileExW(path, old, MOVEFILE_REPLACE_EXISTING);
	InitializeCriticalSection(&log_cs);
	log_file = CreateFileW(path, GENERIC_WRITE, FILE_SHARE_READ, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
}

void ShrimpLog(const char* line)
{
	SYSTEMTIME t;
	char ts[24];
	DWORD w;
	size_t n;
	if (log_file == INVALID_HANDLE_VALUE || line == NULL)
		return;
	GetLocalTime(&t);
	snprintf(ts, sizeof(ts), "%02d:%02d:%02d.%03d ", t.wHour, t.wMinute, t.wSecond, t.wMilliseconds);
	n = strlen(line);
	EnterCriticalSection(&log_cs);
	WriteFile(log_file, ts, (DWORD)strlen(ts), &w, NULL);
	WriteFile(log_file, line, (DWORD)n, &w, NULL);
	if (n == 0 || line[n - 1] != '\n')
		WriteFile(log_file, "\r\n", 2, &w, NULL);
	LeaveCriticalSection(&log_cs);
}

/* Zdarzenia dla interfejsu w dzienniku (postęp najwyżej co 5 s); w trybie --auto-disk koniec przebiegu kończy proces. */
void ShrimpLogEvent(const char* json)
{
	static uint64_t last_progress = 0;
	if (strstr(json, "\"event\":\"progress\"") != NULL) {
		uint64_t now = GetTickCount64();
		if (now - last_progress < 5000)
			return;
		last_progress = now;
	} else if (strstr(json, "\"event\":\"folders\"") != NULL || strstr(json, "\"event\":\"devices\"") != NULL) {
		return;
	}
	if (strncmp(json, "{\"id\"", 6) == 0)
		return;
	ShrimpLog(json);
	if (opt_auto_disk >= 0) {
		if (strstr(json, "\"event\":\"done\"") != NULL) {
			ShrimpLog("Shrimp: --auto-disk: gotowe");
			ExitProcess(0);
		}
		if (strstr(json, "\"event\":\"fail\"") != NULL) {
			ShrimpLog("Shrimp: --auto-disk: błąd");
			ExitProcess(3);
		}
	}
}

/* ---------- argumenty ---------- */
void ShrimpInit(void)
{
	int argc = 0;
	wchar_t** argv = CommandLineToArgvW(GetCommandLineW(), &argc);
	for (int i = 1; argv != NULL && i < argc; i++) {
		const wchar_t* a = argv[i];
		const wchar_t* v = (i + 1 < argc) ? argv[i + 1] : NULL;
		if (wcscmp(a, L"--rufus") == 0)
			shrimp_mode = FALSE;
		else if (wcscmp(a, L"--demo") == 0)
			opt_demo = TRUE;
		else if (v != NULL && wcscmp(a, L"--source") == 0)
			WideCharToMultiByte(CP_UTF8, 0, argv[++i], -1, ShrimpLocalIso, sizeof(ShrimpLocalIso), NULL, NULL);
		else if (v != NULL && wcscmp(a, L"--manifest") == 0)
			WideCharToMultiByte(CP_UTF8, 0, argv[++i], -1, ShrimpManifestOverride, 1024, NULL, NULL);
		else if (v != NULL && wcscmp(a, L"--collect") == 0)
			WideCharToMultiByte(CP_UTF8, 0, argv[++i], -1, opt_collect, sizeof(opt_collect), NULL, NULL);
		else if (v != NULL && wcscmp(a, L"--screen") == 0)
			WideCharToMultiByte(CP_UTF8, 0, argv[++i], -1, opt_screen, sizeof(opt_screen), NULL, NULL);
		else if (v != NULL && wcscmp(a, L"--lang") == 0)
			WideCharToMultiByte(CP_UTF8, 0, argv[++i], -1, opt_lang, sizeof(opt_lang), NULL, NULL);
		else if (i + 3 < argc && wcscmp(a, L"--backup-test") == 0)
			ExitProcess(ShrimpUsbBackupTest(argv[i + 1], argv[i + 2], argv[i + 3]));
		else if (v != NULL && wcscmp(a, L"--theme") == 0)
			WideCharToMultiByte(CP_UTF8, 0, argv[++i], -1, opt_theme, sizeof(opt_theme), NULL, NULL);
		else if (v != NULL && wcscmp(a, L"--auto-disk") == 0)
			opt_auto_disk = _wtoi(argv[++i]);
	}
	LocalFree(argv);
	if (shrimp_mode && !opt_collect[0]) {
		log_open();
		uprintf("Shrimp Install %s", SHRIMP_VERSION);
	}
}

/* ---------- odpowiedzi i zdarzenia ---------- */
static void reply(int id, const char* result_json)
{
	char* s;
	size_t n;
	if (id <= 0)
		return;
	n = strlen(result_json ? result_json : "null") + 48;
	s = malloc(n);
	if (s == NULL)
		return;
	snprintf(s, n, "{\"id\":%d,\"ok\":true,\"result\":%s}", id, result_json ? result_json : "null");
	ShrimpEmit(s);
}

static void reply_error(int id, const char* message)
{
	jb b;
	jb_init(&b);
	jb_obj(&b, NULL);
	jb_num(&b, "id", id);
	jb_bool(&b, "ok", FALSE);
	jb_str(&b, "error", message);
	jb_end_obj(&b);
	ShrimpEmit(b.s);
}

uint64_t ShrimpSpeed(uint64_t done, BOOL reset)
{
	static uint64_t t_prev, d_prev;
	static double ema;
	uint64_t now = GetTickCount64();
	if (reset) {
		t_prev = now;
		d_prev = done;
		ema = 0;
		return 0;
	}
	if (now - t_prev >= 1000 && done >= d_prev) {
		double inst = (double)(done - d_prev) * 1000.0 / (double)(now - t_prev);
		ema = (ema > 0) ? ema * 0.7 + inst * 0.3 : inst;
		t_prev = now;
		d_prev = done;
	}
	return (uint64_t)ema;
}

void ShrimpProgressStep(int phase, const char* step, uint64_t done, uint64_t total, uint64_t speed)
{
	static uint64_t last = 0;
	static int last_phase = 0;
	static const char* last_step = NULL;
	char* s;
	uint64_t now = GetTickCount64();
	if (phase == last_phase && step == last_step && done < total && now - last < 150)
		return;
	last = now;
	last_phase = phase;
	last_step = step;
	s = malloc(200);
	if (s == NULL)
		return;
	snprintf(s, 200, "{\"event\":\"progress\",\"phase\":%d,\"done\":%llu,\"total\":%llu,\"speed\":%llu%s%s%s}",
		phase, (unsigned long long)done, (unsigned long long)total, (unsigned long long)speed,
		step ? ",\"step\":\"" : "", step ? step : "", step ? "\"" : "");
	ShrimpEmit(s);
}

void ShrimpProgress(int phase, uint64_t done, uint64_t total, uint64_t speed)
{
	ShrimpProgressStep(phase, NULL, done, total, speed);
}

void ShrimpEmitImage(void)
{
	jb b;
	jb_init(&b);
	jb_obj(&b, NULL);
	jb_str(&b, "event", "image");
	jb_u64(&b, "size", ShrimpManifest.size);
	jb_str(&b, "version", ShrimpManifest.version);
	jb_end_obj(&b);
	ShrimpEmit(b.s);
}

void ShrimpFail(int phase, const char* kind, const char* message)
{
	jb b;
	if (InterlockedExchange(&fail_sent, 1) != 0)
		return;
	uprintf("Shrimp: błąd w fazie %d: %s %s", phase, kind, message ? message : "");
	jb_init(&b);
	jb_obj(&b, NULL);
	jb_str(&b, "event", "fail");
	jb_num(&b, "phase", phase);
	jb_str(&b, "kind", kind);
	if (message != NULL && message[0])
		jb_str(&b, "message", message);
	jb_end_obj(&b);
	ShrimpEmit(b.s);
}

/* ---------- nośniki ---------- */
static const char* kind_name[] = { "usb", "sd", "disk" };

static void devices_json(jb* b)
{
	char name[256], *p;
	jb_arr(b, NULL);
	for (int i = 0; i < MAX_DRIVES && rufus_drive[i].size != 0; i++) {
		static_strcpy(name, rufus_drive[i].display_name ? rufus_drive[i].display_name : rufus_drive[i].name);
		/* „SanDisk Ultra USB Device (E:) [29.8 GB]” → „SanDisk Ultra (E:)” */
		if ((p = strrchr(name, '[')) != NULL) {
			*p = 0;
			while (p > name && p[-1] == ' ')
				*--p = 0;
		}
		if ((p = strstr(name, " USB Device")) != NULL)
			memmove(p, p + 11, strlen(p + 11) + 1);
		jb_obj(b, NULL);
		jb_num(b, "id", rufus_drive[i].index);
		jb_str(b, "name", name);
		jb_str(b, "kind", kind_name[min(rufus_drive[i].shrimp_kind, 2)]);
		jb_u64(b, "size", rufus_drive[i].size);
		jb_end_obj(b);
	}
	jb_end_arr(b);
}

void ShrimpOnDevicesChanged(void)
{
	jb b;
	if (!shrimp_mode || hShrimpWnd == NULL)
		return;
	jb d;
	jb_init(&d);
	devices_json(&d);
	jb_init(&b);
	jb_obj(&b, NULL);
	jb_str(&b, "event", "devices");
	jb_raw(&b, "devices", d.s);
	jb_end_obj(&b);
	jb_free(&d);
	ShrimpEmit(b.s);
}

/* ---------- foldery ---------- */
static uint64_t folder_size[SF_MAX];
static BOOL folder_known[SF_MAX];

static char* folders_json(BOOL sizes)
{
	jb b;
	wchar_t path[MAX_PATH];
	jb_init(&b);
	jb_arr(&b, NULL);
	for (int i = 0; i < SF_MAX; i++) {
		if (!ShrimpFolderPath(i, path, ARRAYSIZE(path)))
			continue;
		jb_obj(&b, NULL);
		jb_str(&b, "key", shrimp_folder_key[i]);
		jb_wstr(&b, "path", path);
		if (sizes && folder_known[i])
			jb_u64(&b, "size", folder_size[i]);
		else
			jb_null(&b, "size");
		jb_end_obj(&b);
	}
	jb_end_arr(&b);
	return b.s;
}

static DWORD WINAPI FoldersThread(LPVOID param)
{
	wchar_t path[MAX_PATH];
	(void)param;
	CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
	for (int i = 0; i < SF_MAX; i++) {
		folder_size[i] = ShrimpFolderPath(i, path, ARRAYSIZE(path)) ? ShrimpTreeSize(path, NULL) : 0;
		folder_known[i] = TRUE;
		char* f = folders_json(TRUE);
		size_t n = strlen(f) + 40;
		char* s = malloc(n);
		if (s != NULL) {
			snprintf(s, n, "{\"event\":\"folders\",\"folders\":%s}", f);
			ShrimpEmit(s);
		}
		free(f);
	}
	return 0;
}

static DWORD WINAPI BitLockerThread(LPVOID param)
{
	(void)param;
	ShrimpBitLockerOn = ShrimpBitLocker();
	if (ShrimpBitLockerOn)
		ShrimpEmit(_strdup("{\"event\":\"bitlocker\",\"on\":true}"));
	return 0;
}

/* ---------- wybór plików i folderów ---------- */
typedef struct { int id; int count; wchar_t* paths[64]; BOOL folder; } pick_job;

static DWORD WINAPI PickSizeThread(LPVOID param)
{
	pick_job* j = param;
	jb b;
	jb_init(&b);
	jb_arr(&b, NULL);
	for (int i = 0; i < j->count; i++) {
		const wchar_t* name = wcsrchr(j->paths[i], L'\\');
		jb_obj(&b, NULL);
		jb_wstr(&b, "path", j->paths[i]);
		jb_wstr(&b, "name", name && name[1] ? name + 1 : j->paths[i]);
		jb_str(&b, "kind", j->folder ? "folder" : "file");
		jb_u64(&b, "size", ShrimpTreeSize(j->paths[i], NULL));
		jb_end_obj(&b);
		CoTaskMemFree(j->paths[i]);
	}
	jb_end_arr(&b);
	reply(j->id, b.s);
	jb_free(&b);
	free(j);
	return 0;
}

static void pick(int id, BOOL folder)
{
	IFileOpenDialog* d = NULL;
	IShellItemArray* items = NULL;
	DWORD opts = 0, n = 0;
	pick_job* j = calloc(1, sizeof(*j));
	if (j == NULL || FAILED(CoCreateInstance(&CLSID_FileOpenDialog, NULL, CLSCTX_INPROC_SERVER, &IID_IFileOpenDialog, (void**)&d))) {
		free(j);
		reply(id, "[]");
		return;
	}
	j->id = id;
	j->folder = folder;
	IFileOpenDialog_GetOptions(d, &opts);
	IFileOpenDialog_SetOptions(d, opts | FOS_ALLOWMULTISELECT | FOS_FORCEFILESYSTEM | FOS_PATHMUSTEXIST | (folder ? FOS_PICKFOLDERS : FOS_FILEMUSTEXIST));
	IFileOpenDialog_SetTitle(d, folder ? L"Dodaj folder" : L"Dodaj plik");
	if (SUCCEEDED(IFileOpenDialog_Show(d, hShrimpWnd)) && SUCCEEDED(IFileOpenDialog_GetResults(d, &items))) {
		IShellItemArray_GetCount(items, &n);
		for (DWORD i = 0; i < n && j->count < 64; i++) {
			IShellItem* it = NULL;
			PWSTR p = NULL;
			if (SUCCEEDED(IShellItemArray_GetItemAt(items, i, &it)) && SUCCEEDED(IShellItem_GetDisplayName(it, SIGDN_FILESYSPATH, &p)))
				j->paths[j->count++] = p;
			if (it)
				IShellItem_Release(it);
		}
		IShellItemArray_Release(items);
	}
	IFileOpenDialog_Release(d);
	if (CreateThread(NULL, 0, PickSizeThread, j, 0, NULL) == NULL) {
		reply(id, "[]");
		free(j);
	}
}

/* ---------- przebieg ---------- */
static void begin(int id, char** f, int nf)
{
	shrimp_transfer* t = &ShrimpTransfer;
	wchar_t path[MAX_PATH];

	if (state == ST_FETCH || state == ST_SCAN || state == ST_FORMAT || state == ST_COPY) {
		reply_error(id, "busy");
		return;
	}
	for (int i = 0; i < t->count; i++)
		free(t->items[i].source);
	memset(t, 0, sizeof(*t));
	plan_device = (nf > 2) ? (DWORD)strtoul(f[2], NULL, 10) : 0;
	t->DriveIndex = plan_device;
	/* Moje foldery: klucze po przecinku. */
	if (nf > 3 && f[3][0]) {
		char keys[256], *tok, *ctx = NULL;
		static_strcpy(keys, f[3]);
		for (tok = strtok_s(keys, ",", &ctx); tok && t->count < 64; tok = strtok_s(NULL, ",", &ctx)) {
			for (int k = 0; k < SF_MAX; k++) {
				if (strcmp(tok, shrimp_folder_key[k]) == 0 && ShrimpFolderPath(k, path, ARRAYSIZE(path))) {
					shrimp_item* it = &t->items[t->count++];
					it->source = _wcsdup(path);
					static_strcpy(it->key, shrimp_folder_key[k]);
					static_sprintf(it->dest, "files/%s", shrimp_folder_key[k]);
				}
			}
		}
	}
	/* Wybiorę sam: ścieżki, do files/other/<nazwa> (bez powtórzeń nazw). */
	for (int i = 4; i < nf && t->count < 64; i++) {
		shrimp_item* it = &t->items[t->count];
		const char* base = strrchr(f[i], '\\');
		char name[MAX_PATH];
		base = base ? base + 1 : f[i];
		static_strcpy(name, base);
		for (int k = 2; ; k++) {
			BOOL dup = FALSE;
			for (int j = 0; j < t->count; j++)
				if (_stricmp(t->items[j].dest + (strncmp(t->items[j].dest, "files/other/", 12) == 0 ? 12 : 0), name) == 0)
					dup = TRUE;
			if (!dup)
				break;
			static_sprintf(name, "%s (%d)", base, k);
		}
		it->source = utf8_to_wchar(f[i]);
		if (it->source == NULL)
			continue;
		static_strcpy(it->key, "other");
		static_sprintf(it->dest, "files/other/%s", name);
		t->count++;
	}

	ShrimpCancel = FALSE;
	fail_sent = 0;
	last_error[0] = 0;
	refused_kind = NULL;
	state = ST_FETCH;
	reply(id, "null");
	if (CreateThread(NULL, 0, ShrimpFetchThread, NULL, 0, NULL) == NULL) {
		state = ST_FAILED;
		ShrimpFail(1, "other", "startFail");
	}
}

static int device_row(DWORD index)
{
	for (int i = 0; i < ComboBox_GetCount(hDeviceList); i++)
		if ((DWORD)ComboBox_GetItemData(hDeviceList, i) == index)
			return i;
	return -1;
}

static BOOL select_data(HWND combo, int ctrl, int data)
{
	for (int i = 0; i < ComboBox_GetCount(combo); i++) {
		if ((int)ComboBox_GetItemData(combo, i) == data) {
			if (ComboBox_GetCurSel(combo) != i) {
				ComboBox_SetCurSel(combo, i);
				SendMessage(hMainDialog, WM_COMMAND, MAKEWPARAM(ctrl, CBN_SELCHANGE), (LPARAM)combo);
			}
			return TRUE;
		}
	}
	return FALSE;
}

static void finish_cancel(void)
{
	state = ST_IDLE;
	if (cancel_id) {
		reply(cancel_id, "null");
		cancel_id = 0;
	}
}

static void cancel(int id)
{
	if (state != ST_FETCH && state != ST_SCAN && state != ST_BACKUP && state != ST_FORMAT && state != ST_COPY) {
		state = ST_IDLE;
		reply(id, "null");
		return;
	}
	cancel_id = id;
	ShrimpCancel = TRUE;
	if (state == ST_FORMAT && (format_thread != NULL || op_in_progress)) {
		no_confirmation_on_cancel = TRUE;
		SendMessage(hMainDialog, WM_COMMAND, IDCANCEL, 0);
	}
}

/* Strona pomocy w przeglądarce: tylko https:// i zwykłe znaki adresu, żeby wiadomość z interfejsu nie mogła
 * uruchomić niczego innego (ShellExecute otwiera też pliki i programy). */
static void open_help_url(const char* url)
{
	size_t i, n = strlen(url);
	if (n < 12 || n > 500 || strncmp(url, "https://", 8) != 0)
		return;
	for (i = 0; i < n; i++) {
		char c = url[i];
		if (!((c >= 'a' && c <= 'z') || (c >= 'A' && c <= 'Z') || (c >= '0' && c <= '9') || strchr(".-_~:/?&=%", c) != NULL))
			return;
	}
	wchar_t* w = utf8_to_wchar(url);
	if (w != NULL)
		ShellExecuteW(NULL, L"open", w, NULL, NULL, SW_SHOWNORMAL);
	free(w);
}

LRESULT ShrimpWndMessage(UINT msg, WPARAM wParam, LPARAM lParam)
{
	switch (msg) {
	case WM_SHRIMP_CALL: {
		char* m = (char*)lParam;
		char* f[72];
		int nf = 0;
		for (char *p = m, *s = m; nf < 72; p++) {
			if (*p == '\t' || *p == 0) {
				BOOL end = (*p == 0);
				*p = 0;
				f[nf++] = s;
				s = p + 1;
				if (end)
					break;
			}
		}
		int id = nf > 0 ? atoi(f[0]) : 0;
		const char* call = nf > 1 ? f[1] : "";
		if (strcmp(call, "start") == 0 || strcmp(call, "cancel") == 0 || strcmp(call, "reboot") == 0)
			uprintf("Shrimp: wywołanie %s (stan %d)", call, state);
		if (strcmp(call, "info") == 0) {
			jb b;
			char* fj = folders_json(TRUE);
			jb_init(&b);
			jb_obj(&b, NULL);
			jb_bool(&b, "bitlocker", ShrimpBitLockerOn);
			jb_u64(&b, "shrimpSize", ShrimpManifest.size ? ShrimpManifest.size : SHRIMP_ISO_ESTIMATE);
			jb_u64(&b, "minSize", SHRIMP_MIN_MEDIA_SIZE);
			jb_raw(&b, "folders", fj);
			jb_end_obj(&b);
			reply(id, b.s);
			jb_free(&b);
			free(fj);
			if (!folders_started) {
				folders_started = TRUE;
				CreateThread(NULL, 0, FoldersThread, NULL, 0, NULL);
			}
		} else if (strcmp(call, "devices") == 0) {
			jb b;
			jb_init(&b);
			devices_json(&b);
			reply(id, b.s);
			jb_free(&b);
		} else if (strcmp(call, "pick") == 0) {
			pick(id, nf > 2 && strcmp(f[2], "folder") == 0);
		} else if (strcmp(call, "start") == 0) {
			begin(id, f, nf);
		} else if (strcmp(call, "cancel") == 0) {
			cancel(id);
		} else if (strcmp(call, "reboot") == 0) {
			const char* err = ShrimpSetBootNext(plan_device);
			if (err != NULL) {
				reply_error(id, err);
			} else {
				ShrimpDisableFastStartup();
				if (ShrimpReboot())
					reply(id, "null");
				else
					reply_error(id, "rebootFail");
			}
		} else if (strcmp(call, "open") == 0) {
			if (nf > 2)
				open_help_url(f[2]);
		} else if (strcmp(call, "later") == 0 || strcmp(call, "close") == 0) {
			PostMessage(hShrimpWnd, WM_CLOSE, 0, 0);
		} else if (strcmp(call, "minimize") == 0) {
			ShowWindow(hShrimpWnd, SW_MINIMIZE);
		} else if (strcmp(call, "drag") == 0) {
			ReleaseCapture();
			SendMessage(hShrimpWnd, WM_NCLBUTTONDOWN, HTCAPTION, 0);
		}
		free(m);
		return 0;
	}

	case WM_SHRIMP_FETCHED:
		if (ShrimpCancel) {
			finish_cancel();
			break;
		}
		if (!wParam) {
			state = ST_FAILED;
			ShrimpFail(1, "other", NULL);
			break;
		}
		/* Nośnik wybrany w interfejsie → ten sam w Rufusie, potem analiza ISO. */
		{
			int row = device_row(plan_device);
			if (row < 0) {
				state = ST_FAILED;
				ShrimpFail(2, "device", NULL);
				break;
			}
			ComboBox_SetCurSel(hDeviceList, row);
			SendMessage(hMainDialog, WM_COMMAND, MAKEWPARAM(IDC_DEVICE, CBN_SELCHANGE), (LPARAM)hDeviceList);
		}
		uprintf("Shrimp: obraz gotowy (%s), analiza w Rufusie", ShrimpIsoPath);
		state = ST_SCAN;
		ShrimpProgressStep(2, "prepare", 0, 0, 0);
		free(image_path);
		image_path = _strdup(ShrimpIsoPath);
		ErrorStatus = 0;
		if (CreateThread(NULL, 0, ImageScanThread, NULL, 0, NULL) == NULL) {
			state = ST_FAILED;
			ShrimpFail(2, "write", NULL);
		}
		break;

	case WM_SHRIMP_SCANNED:
		if (ShrimpCancel) {
			finish_cancel();
			break;
		}
		if (!wParam) {
			state = ST_FAILED;
			ShrimpFail(2, "write", "imageBad");
			break;
		}
		/* Najpierw zawartość nośnika: bez sprawdzonej kopii nic nie zostanie sformatowane. */
		uprintf("Shrimp: analiza obrazu skończona, kopia zawartości nośnika");
		state = ST_BACKUP;
		ShrimpSpeed(0, TRUE);
		if (CreateThread(NULL, 0, ShrimpUsbBackupThread, (LPVOID)(uintptr_t)plan_device, 0, NULL) == NULL) {
			state = ST_FAILED;
			ShrimpFail(2, "copy", "backupRead");
		}
		break;

	case WM_SHRIMP_BACKED_UP:
		if (ShrimpCancel) {
			finish_cancel();
			break;
		}
		if (!wParam) {
			state = ST_FAILED;
			ShrimpFail(2, "copy", "backupRead");   /* zwykle już zgłoszone dokładniej */
			break;
		}
		/* Nośnik jak dla Shrimp: GPT, UEFI, FAT32 (Setup dopisuje pliki z Windows). */
		select_data(hPartitionScheme, IDC_PARTITION_TYPE, PARTITION_STYLE_GPT);
		select_data(hTargetSystem, IDC_TARGET_SYSTEM, TT_UEFI);
		if (!img_report.has_4GB_file)
			select_data(hFileSystem, IDC_FILE_SYSTEM, FS_FAT32);
		else
			uprintf("Shrimp: UWAGA: obraz ma plik > 4 GB, Rufus wybierze inny system plików");
		/* Zawsze szybkie formatowanie, bez testu uszkodzonych bloków (pełne formatowanie 32 GB to kilkadziesiąt minut). */
		CheckDlgButton(hMainDialog, IDC_QUICK_FORMAT, BST_CHECKED);
		CheckDlgButton(hMainDialog, IDC_BAD_BLOCKS, BST_UNCHECKED);
		ShrimpUsbBackupMarkFormatted();
		uprintf("Shrimp: kopia zawartości nośnika gotowa (%llu B), START w Rufusie", (unsigned long long)ShrimpUsbBackupBytes);
		state = ST_FORMAT;
		copied_bytes = 0;
		ShrimpSpeed(0, TRUE);
		ShrimpProgressStep(2, "prepare", 0, 0, 0);
		SendMessage(hMainDialog, WM_COMMAND, IDC_START, 0);
		break;

	case WM_SHRIMP_FORMATTED:
		if (refused_kind != NULL && !ShrimpCancel) {
			/* Zapis zatrzymany na pytaniu Rufusa (np. inny program używa nośnika). */
			state = ST_FAILED;
			/* Tekst Rufusa tylko przy nieznanym pytaniu; „nośnik jest używany” ma własny opis w interfejsie. */
			ShrimpFail(2, refused_kind, strcmp(refused_kind, "question") == 0 ? refused_text : NULL);
			refused_kind = NULL;
			break;
		}
		if (ShrimpCancel) {
			finish_cancel();
			break;
		}
		/* Rufus przerwał przed zapisem (np. obraz niezgodny z nośnikiem): bez tego interfejs czekałby bez końca. */
		if (SCODE_CODE((DWORD)wParam) == ERROR_CANCELLED) {
			uprintf("Shrimp: Rufus przerwał zapis przed startem: %s", last_error[0] ? last_error : "(bez komunikatu)");
			state = ST_FAILED;
			ShrimpFail(2, "write", last_error[0] ? last_error : NULL);
			break;
		}
		if (IS_ERROR((DWORD)wParam)) {
			state = ST_FAILED;
			ShrimpFail(2, "write", last_error[0] ? last_error : NULL);
			break;
		}
		uprintf("Shrimp: zapis obrazu skończony, kopia plików");
		ShrimpProgress(2, 100, 100, 0);
		/* Pobrany obraz już niepotrzebny (zostawiamy tylko wskazany przez --source). */
		if (!ShrimpLocalIso[0])
			DeleteFileU(ShrimpIsoPath);
		state = ST_COPY;
		if (CreateThread(NULL, 0, ShrimpCopyThread, NULL, 0, NULL) == NULL) {
			state = ST_FAILED;
			ShrimpFail(3, "copy", NULL);
		}
		break;

	case WM_SHRIMP_COPIED:
		if (ShrimpCancel) {
			finish_cancel();
			break;
		}
		if (!wParam) {
			state = ST_FAILED;
			ShrimpFail(3, "copy", NULL);
			break;
		}
		state = ST_DONE;
		ShrimpEmit(_strdup("{\"event\":\"done\"}"));
		break;
	}
	return 0;
}

void ShrimpDispatch(const char* msg)
{
	/* Obsługa poza wywołaniem zwrotnym WebView2 (okna dialogowe, długie operacje). */
	char* m = _strdup(msg);
	if (m != NULL && !PostMessage(hShrimpWnd, WM_SHRIMP_CALL, 0, (LPARAM)m))
		free(m);
}

BOOL ShrimpCanClose(void)
{
	return state != ST_FETCH && state != ST_SCAN && state != ST_BACKUP && state != ST_FORMAT && state != ST_COPY;
}

/* ---------- punkty zaczepienia w Rufusie ---------- */


static void refuse(const char* kind, const char* title, const char* message)
{
	refused_kind = kind;
	static_sprintf(refused_text, "%s%s%s", title ? title : "", title && message ? ": " : "", message ? message : "");
	for (char* p = refused_text; *p; p++)
		if (*p == '\r' || *p == '\n')
			*p = ' ';
	uprintf("Shrimp: pytanie Rufusa bez zgody (%s)", kind);
}

/* Czy tekst powstał z komunikatu Rufusa `id` z jednym argumentem %s (porównanie części przed i po argumencie). */
static BOOL matches_msg1(const char* text, int id)
{
	char fmt[LOC_MESSAGE_SIZE];
	const char *mark, *tail;
	size_t head;
	if (text == NULL)
		return FALSE;
	static_strcpy(fmt, lmprintf(id, "\x01"));
	mark = strchr(fmt, '\x01');
	if (mark == NULL)
		return strcmp(text, fmt) == 0;
	head = (size_t)(mark - fmt);
	tail = mark + 1;
	return strncmp(text, fmt, head) == 0 && strlen(text) >= head + strlen(tail)
		&& strcmp(text + strlen(text) - strlen(tail), tail) == 0;
}

/* Odpowiedzi na pytania Rufusa. Tylko pytania znane z przebiegu Shrimp (nośnik GPT + UEFI + FAT32, tryb ISO)
 * dostają zgodę; każde inne zatrzymuje zapis i pokazuje użytkownikowi tekst Rufusa, zamiast zgadywać. */
int ShrimpNotification(int type, const char* title_in, const char* message)
{
	char title[256];
	int kind = type & 0x0F, no;
	static_strcpy(title, title_in ? title_in : "");
	uprintf("Shrimp: komunikat Rufusa: %s: %s", title, message ? message : "");
	if ((type & MB_ICONMASK) == MB_ICONERROR && message != NULL) {
		static_strcpy(last_error, message);
		for (char* p = last_error; *p; p++)
			if (*p == '\r' || *p == '\n')
				*p = ' ';
	}
	if (kind != MB_OKCANCEL && kind != MB_YESNO && kind != MB_YESNOCANCEL && kind != MB_ABORTRETRYIGNORE)
		return IDOK;	/* informacja albo błąd: tylko potwierdzenie */
	no = kind == MB_OKCANCEL ? IDCANCEL : kind == MB_ABORTRETRYIGNORE ? IDABORT : IDNO;

	/* Inny program używa nośnika: nie formatujemy na siłę, użytkownik zamyka program i próbuje jeszcze raz. */
	if (message != NULL && strcmp(message, lmprintf(MSG_132)) == 0) {
		refuse("busy", NULL, message);
		return no;
	}
	/* „Wszystkie dane na nośniku zostaną usunięte”: zawartość jest już skopiowana i sprawdzona (usbbackup.c). */
	if (kind == MB_OKCANCEL && strcmp(title, APPLICATION_NAME) == 0)
		return IDOK;
	/* Kilka partycji na nośniku, sektor inny niż 512 B: formatujemy cały nośnik, Rufus tylko ostrzega. */
	if (kind == MB_OKCANCEL && (strcmp(title, lmprintf(MSG_094)) == 0 || strcmp(title, lmprintf(MSG_197)) == 0))
		return IDOK;
	/* Pobieranie plików GRUB/Syslinux/DBX z serwerów Rufusa: niepotrzebne dla startu z UEFI, odmawiamy.
	 * Tam, gdzie „nie” znaczy „użyj wersji wbudowanej” (pytanie z trzema odpowiedziami), zapis idzie dalej;
	 * gdzie „nie” przerywa zapis, użytkownik dostaje tekst Rufusa zamiast cichego przerwania. */
	if (kind == MB_YESNOCANCEL && (strcmp(title, lmprintf(MSG_115)) == 0 || matches_msg1(title, MSG_103)))
		return IDNO;
	if (strcmp(title, lmprintf(MSG_353)) == 0)
		return IDNO;
	/* Starsze pliki .c32 z obrazu: Rufus pyta, czy je podmienić; zostawiamy pliki z obrazu Shrimp. */
	if (matches_msg1(title, MSG_083))
		return IDNO;
	/* Wszystko inne, np. odwołany program rozruchowy UEFI w obrazie (MSG_338: nośnik nie wystartuje z Secure Boot):
	 * zapis się zatrzymuje, a interfejs pokazuje pytanie Rufusa jako błąd. */
	refuse("question", title, message);
	return no;
}

int ShrimpSelection(const char* title, const char* message, int choices)
{
	(void)choices;
	/* Pierwsza opcja: dla obrazów ISOHybrid to tryb ISO (pliki na FAT32, miejsce na kopię z Windows). */
	uprintf("Shrimp: wybór Rufusa (pierwsza opcja): %s: %s", title ? title : "", message ? message : "");
	return 1;
}

void ShrimpOnImageScanned(BOOL ok)
{
	if (shrimp_mode && hShrimpWnd != NULL && state == ST_SCAN)
		PostMessage(hShrimpWnd, WM_SHRIMP_SCANNED, ok && img_report.is_iso, 0);
}

/* Pasek fazy 2 to pasek Rufusa (formatowanie i zapis plików obrazu) przeliczony na bajty obrazu.
 * Prędkość tylko z bajtów naprawdę zapisanych na nośnik (kopia plików ISO), nie z przeskoków paska
 * po formatowaniu: inaczej wychodziły bzdury w rodzaju 1116 MB/s. Do pierwszych bajtów: krok „prepare”. */
void ShrimpOnFileCopy(uint64_t processed, uint64_t total)
{
	(void)total;
	if (shrimp_mode && state == ST_FORMAT)
		copied_bytes = processed * 2048;
}

void ShrimpOnProgress(int pos, int max)
{
	uint64_t size, done;
	if (!shrimp_mode || state != ST_FORMAT || max <= 0)
		return;
	size = ShrimpManifest.size ? ShrimpManifest.size : SHRIMP_ISO_ESTIMATE;
	done = size * (uint64_t)pos / (uint64_t)max;
	if (copied_bytes == 0) {
		speed_armed = FALSE;
		ShrimpProgressStep(2, "prepare", done, size, 0);
		return;
	}
	if (!speed_armed) {
		/* Pomiar od pierwszych zapisanych bajtów, bez czasu formatowania. */
		speed_armed = TRUE;
		ShrimpSpeed(copied_bytes, TRUE);
	}
	ShrimpProgress(2, done, size, ShrimpSpeed(copied_bytes, FALSE));
}

void ShrimpOnFormatDone(DWORD error_status)
{
	if (shrimp_mode && hShrimpWnd != NULL && state == ST_FORMAT)
		PostMessage(hShrimpWnd, WM_SHRIMP_FORMATTED, (WPARAM)error_status, 0);
}

static void collect_only(void)
{
	jb b;
	HANDLE h;
	DWORD w;
	char* f;
	CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
	ShrimpBitLockerOn = ShrimpBitLocker();
	for (int i = 0; i < SF_MAX; i++) {
		wchar_t path[MAX_PATH];
		folder_size[i] = ShrimpFolderPath(i, path, ARRAYSIZE(path)) ? ShrimpTreeSize(path, NULL) : 0;
		folder_known[i] = TRUE;
	}
	jb_init(&b);
	jb_obj(&b, NULL);
	jb_num(&b, "version", 1);
	jb_str(&b, "installer", "Shrimp Install " SHRIMP_VERSION);
	ShrimpCollect(&b);
	f = folders_json(TRUE);
	jb_raw(&b, "folders", f);
	free(f);
	{
		jb d;
		jb_init(&d);
		devices_json(&d);
		jb_raw(&b, "devices", d.s);
		jb_free(&d);
	}
	jb_end_obj(&b);
	h = CreateFileU(opt_collect, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
	if (h != INVALID_HANDLE_VALUE) {
		WriteFile(h, b.s, (DWORD)b.len, &w, NULL);
		CloseHandle(h);
	}
	jb_free(&b);
	ExitProcess(h != INVALID_HANDLE_VALUE ? 0 : 1);
}

BOOL ShrimpStart(HINSTANCE hInstance, HWND hMainDlg)
{
	jb o;
	(void)hMainDlg;
	if (!shrimp_mode)
		return FALSE;
	/* Pendrive'y, karty SD i dyski zewnętrzne USB (dyski wewnętrzne Rufus pomija zawsze). */
	enable_HDDs = TRUE;
	GetDevices(0);
	if (opt_collect[0])
		collect_only();

	jb_init(&o);
	jb_obj(&o, NULL);
	jb_bool(&o, "demo", opt_demo);
	if (opt_screen[0])
		jb_str(&o, "screen", opt_screen);
	if (opt_theme[0])
		jb_str(&o, "theme", opt_theme);
	/* Język: --lang wygrywa, w innym wypadku języki interfejsu Windows (w kolejności preferencji użytkownika). */
	if (opt_lang[0])
		jb_str(&o, "lang", opt_lang);
	{
		ULONG n = 0, len = 0;
		if (GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, NULL, &len) && len > 1) {
			wchar_t* buf = calloc(len, sizeof(wchar_t));
			if (buf != NULL && GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, buf, &len)) {
				jb_arr(&o, "langs");
				for (const wchar_t* p = buf; *p; p += wcslen(p) + 1) {
					char u[64];
					if (WideCharToMultiByte(CP_UTF8, 0, p, -1, u, sizeof(u), NULL, NULL) > 0)
						jb_str(&o, NULL, u);
				}
				jb_end_arr(&o);
			}
			free(buf);
		}
	}
	jb_end_obj(&o);
	BOOL ok = ShrimpWindowCreate(hInstance, o.s);
	jb_free(&o);
	if (ok && !opt_demo)
		CreateThread(NULL, 0, BitLockerThread, NULL, 0, NULL);
	/* Test w CI: przebieg od razu, jak po „Przygotuj nośnik” w interfejsie (bez folderów użytkownika). */
	if (ok && opt_auto_disk >= 0) {
		char* m = malloc(64);
		if (m != NULL) {
			snprintf(m, 64, "0\tstart\t%lu\t", (unsigned long)(DRIVE_INDEX_MIN + opt_auto_disk));
			uprintf("Shrimp: --auto-disk %d", opt_auto_disk);
			if (!PostMessage(hShrimpWnd, WM_SHRIMP_CALL, 0, (LPARAM)m))
				free(m);
		}
	}
	return TRUE;
}
