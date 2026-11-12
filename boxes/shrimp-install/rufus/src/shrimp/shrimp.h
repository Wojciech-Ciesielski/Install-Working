/*
 * Shrimp Install: fork Rufusa z interfejsem WebView2 (ui/ w repozytorium Shrimp).
 * Copyright © 2026 Shrimp. GPLv3, jak Rufus.
 *
 * Rufus robi to, co umie najlepiej (wykrywanie nośników, analiza ISO, formatowanie i zapis).
 * Jego główne okno istnieje, ale jest ukryte: Shrimp ustawia jego kontrolki i „naciska” START.
 * Wszystkie miejsca, w których Rufus woła Shrimp, są oznaczone w kodzie Rufusa komentarzem SHRIMP.
 */
#pragma once

#include <windows.h>
#include <stdint.h>

#define SHRIMP_VERSION "0.1.0"
#define SHRIMP_MIN_MEDIA_SIZE (14ULL * 1000 * 1000 * 1000)   /* pendrive „16 GB” ma naprawdę 14,5–16 mld bajtów; interfejs pisze „16 GB” */
#define SHRIMP_ISO_ESTIMATE (2000ULL * 1024 * 1024)        /* zanim przyjdzie manifest (Beta 2: 1,94 GiB) */
#define SHRIMP_TRANSFER_DIR "shrimp-transfer"

/* ---------- punkty zaczepienia w Rufusie ---------- */
extern BOOL shrimp_mode;
/* Początek WinMain: czyta argumenty, tryby bez okna (--collect) kończą tu proces. */
void ShrimpInit(void);
/* Zamiast pokazania głównego okna Rufusa. FALSE = pokaż Rufusa (--rufus). */
BOOL ShrimpStart(HINSTANCE hInstance, HWND hMainDlg);
/* NotificationEx/SelectionDialog/ListDialog w trybie Shrimp nie pokazują okien. */
int ShrimpNotification(int type, const char* title, const char* message);
int ShrimpSelection(const char* title, const char* message, int choices);
void ShrimpOnDevicesChanged(void);
void ShrimpOnImageScanned(BOOL ok);
void ShrimpOnProgress(int pos, int max);
void ShrimpOnFormatDone(DWORD error_status);
/* _UpdateProgressWithInfo(OP_FILE_COPY): bloki ISO (2048 B) skopiowane na nośnik, do liczenia prędkości. */
void ShrimpOnFileCopy(uint64_t processed, uint64_t total);
/* uprintf: każdy wiersz dziennika Rufusa i Shrimp trafia też do %LOCALAPPDATA%\Shrimp Install\shrimp-install.log. */
void ShrimpLog(const char* line);
void ShrimpLogEvent(const char* json);    /* zdarzenie dla interfejsu (window.c) */

/* ---------- wewnętrzne ---------- */
/* json.c: budowanie JSON w UTF-8 */
typedef struct { char* s; size_t len, cap; int depth; BOOL first[32]; } jb;
void jb_init(jb* b);
void jb_free(jb* b);
void jb_text(jb* b, const char* s);   /* dopisuje tekst bez zmian */
void jb_obj(jb* b, const char* key);        /* key NULL wewnątrz tablicy */
void jb_end_obj(jb* b);
void jb_arr(jb* b, const char* key);
void jb_end_arr(jb* b);
void jb_str(jb* b, const char* key, const char* v);
void jb_wstr(jb* b, const char* key, const wchar_t* v);
void jb_num(jb* b, const char* key, double v);
void jb_u64(jb* b, const char* key, uint64_t v);
void jb_bool(jb* b, const char* key, BOOL v);
void jb_null(jb* b, const char* key);
void jb_raw(jb* b, const char* key, const char* raw);
/* Prosty odczyt płaskiego JSON (manifest): kopiuje wartość tekstową albo liczbową klucza. */
BOOL json_get(const char* json, const char* key, char* out, size_t out_size);

/* window.c: okno i WebView2 */
#define WM_SHRIMP_EMIT     (WM_APP + 0x300)   /* lParam: char* JSON (malloc) */
#define WM_SHRIMP_CALL     (WM_APP + 0x301)   /* lParam: funkcja do wykonania w wątku okna */
#define WM_SHRIMP_SCANNED  (WM_APP + 0x302)
#define WM_SHRIMP_FORMATTED (WM_APP + 0x303)
#define WM_SHRIMP_FETCHED  (WM_APP + 0x304)   /* wParam: TRUE gdy ISO gotowe */
#define WM_SHRIMP_COPIED   (WM_APP + 0x305)   /* wParam: TRUE gdy kopia skończona */
#define WM_SHRIMP_BACKED_UP (WM_APP + 0x306)  /* wParam: TRUE gdy zawartość nośnika jest bezpieczna w kopii */
extern HWND hShrimpWnd;
BOOL ShrimpWindowCreate(HINSTANCE hInstance, const char* opts_json);
void ShrimpWindowCleanup(void);
/* Wysyła JSON do interfejsu; można wołać z dowolnego wątku (przejmuje json). */
void ShrimpEmit(char* json);
void ShrimpDispatch(const char* msg);       /* shrimp.c: wiadomość z interfejsu */
LRESULT ShrimpWndMessage(UINT msg, WPARAM wParam, LPARAM lParam); /* shrimp.c: WM_SHRIMP_* */
BOOL ShrimpCanClose(void);

/* collect.c: dane z Windows */
typedef enum { SF_DOCUMENTS, SF_PICTURES, SF_DESKTOP, SF_MUSIC, SF_VIDEOS, SF_DOWNLOADS, SF_MAX } shrimp_folder;
extern const char* shrimp_folder_key[SF_MAX];
BOOL ShrimpFolderPath(shrimp_folder f, wchar_t* path, size_t size);
/* Rozmiar drzewa z pominięciem dowiązań i plików tylko w chmurze. */
uint64_t ShrimpTreeSize(const wchar_t* path, volatile BOOL* cancel);
BOOL ShrimpBitLocker(void);
/* Pełny zbiór danych do shrimp.json (bez plików): windows, user, locale, apps. */
void ShrimpCollect(jb* b);

/* transfer.c: znacznik języka Shrimp (np. „pt-br”, „zh-hant”) z nazwy języka Windows (BCP-47). */
void ShrimpLangTag(const char* bcp, char* out, size_t n);

/* fetch.c: obraz Shrimp */
typedef struct {
	char version[64];
	char url[1024];
	char sha256[65];
	uint64_t size;
} shrimp_manifest;
extern shrimp_manifest ShrimpManifest;
extern char ShrimpIsoPath[MAX_PATH];
extern volatile BOOL ShrimpCancel;
/* Wątek: manifest, pobranie, SHA-256. Kończy WM_SHRIMP_FETCHED. Błąd: ShrimpFail(). */
DWORD WINAPI ShrimpFetchThread(LPVOID param);
const char* ShrimpManifestUrl(void);
extern char ShrimpManifestOverride[1024];
extern BOOL ShrimpBitLockerOn;
extern char ShrimpLocalIso[MAX_PATH];

/* transfer.c: kopia plików na nośnik i shrimp.json */
typedef struct {
	wchar_t* source;           /* malloc */
	char dest[MAX_PATH];       /* względem shrimp-transfer/, np. files/documents */
	char key[16];
} shrimp_item;
typedef struct {
	shrimp_item items[64];
	int count;
	DWORD DriveIndex;          /* nośnik; litera dysku ustalana po formatowaniu */
} shrimp_transfer;
extern shrimp_transfer ShrimpTransfer;
DWORD WINAPI ShrimpCopyThread(LPVOID param);

/* usbbackup.c: zawartość nośnika wraca na niego po instalacji (shrimp-backup/, Setup) */
extern uint64_t ShrimpUsbBackupBytes;      /* bajty plików z nośnika (0: pusty) */
/* Wątek (param: DriveIndex): kopia w Windows ze sprawdzeniem sum. Kończy WM_SHRIMP_BACKED_UP. Błąd: ShrimpFail(). */
DWORD WINAPI ShrimpUsbBackupThread(LPVOID param);
void ShrimpUsbBackupMarkFormatted(void);
/* Kopia do <nośnik>\shrimp-backup, odczyt i porównanie sum, potem usunięcie kopii w Windows. */
BOOL ShrimpUsbWriteBack(const wchar_t* media_root, uint64_t* done, uint64_t total);
int ShrimpUsbBackupTest(const wchar_t* src, const wchar_t* dst, const wchar_t* work);

/* boot.c: jednorazowy start z nośnika */
/* Zwraca NULL albo opis błędu (po polsku, dla użytkownika). */
const char* ShrimpSetBootNext(DWORD DriveIndex);
void ShrimpDisableFastStartup(void);
BOOL ShrimpReboot(void);

/* shrimp.c: wspólne */
void ShrimpProgress(int phase, uint64_t done, uint64_t total, uint64_t speed);
/* Jak wyżej, z krokiem w fazie: „verify” (suma pobranego już obrazu), „prepare” (analiza, formatowanie),
 * „backup” (kopia zawartości nośnika); NULL: główna praca fazy (pobieranie, zapis plików obrazu, kopia). */
void ShrimpProgressStep(int phase, const char* step, uint64_t done, uint64_t total, uint64_t speed);
/* Zdarzenie „image”: prawdziwy rozmiar i wersja obrazu (po manifeście albo --source). */
void ShrimpEmitImage(void);
void ShrimpFail(int phase, const char* kind, const char* message);
uint64_t ShrimpSpeed(uint64_t done, BOOL reset);
