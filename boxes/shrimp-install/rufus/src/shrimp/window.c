/*
 * Shrimp Install: okno 1000×700 z WebView2 (bez paska tytułu Windows; pasek rysuje interfejs).
 *
 * WebView2Loader.dll jest wkompilowany jako zasób i wypakowany do folderu tymczasowego.
 * Interfejs (ui/dist/index.html, jeden plik) też jest zasobem i trafia do NavigateToString.
 * Wiadomości: interfejs → program: tekst „id⇥wywołanie⇥argumenty”; program → interfejs: JSON.
 *
 * Copyright © 2026 Shrimp. GPLv3.
 */
#define COBJMACROS
#include <windows.h>
#include <dwmapi.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <wchar.h>
#include "WebView2.h"
#include "native_i18n.h"
#include "shrimp.h"
#include "shrimp_res.h"
#include "../rufus.h"
#include "../msapi_utf8.h"
#include "../resource.h"

#define WIN_W 1000
#define WIN_H 700
static const wchar_t cls_name[] = L"ShrimpInstallWindow";

HWND hShrimpWnd = NULL;
static ICoreWebView2Environment* env = NULL;
static ICoreWebView2Controller* controller = NULL;
static ICoreWebView2* webview = NULL;
static wchar_t* opts_script = NULL;
static wchar_t tmp_dir[MAX_PATH];
static BOOL dark = FALSE;

/* ---------- pomocnicze ---------- */
static wchar_t* utf8_to_w(const char* s, int len)
{
	int n = MultiByteToWideChar(CP_UTF8, 0, s, len, NULL, 0);
	wchar_t* w = malloc((n + 1) * sizeof(wchar_t));
	if (w == NULL)
		return NULL;
	MultiByteToWideChar(CP_UTF8, 0, s, len, w, n);
	w[n] = 0;
	return w;
}

static const BYTE* resource(int id, DWORD* size)
{
	HRSRC r = FindResourceW(hMainInstance, MAKEINTRESOURCEW(id), (LPCWSTR)RT_RCDATA);
	HGLOBAL g = r ? LoadResource(hMainInstance, r) : NULL;
	*size = r ? SizeofResource(hMainInstance, r) : 0;
	return g ? LockResource(g) : NULL;
}

/* Komunikat sprzed startu interfejsu w języku Windows (native_i18n.h z boxes/shrimp-install/i18n), inaczej po angielsku. */
static const wchar_t* native_tr(int id)
{
	static int lang = -1;
	if (lang < 0) {
		ULONG n = 0, len = 0;
		char u[64], tag[32];
		lang = 0;
		if (GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, NULL, &len) && len > 1) {
			wchar_t* buf = calloc(len, sizeof(wchar_t));
			if (buf != NULL && GetUserPreferredUILanguages(MUI_LANGUAGE_NAME, &n, buf, &len) &&
				WideCharToMultiByte(CP_UTF8, 0, buf, -1, u, sizeof(u), NULL, NULL) > 0) {
				ShrimpLangTag(u, tag, sizeof(tag));
				for (int i = 0; i < (int)ARRAYSIZE(SHRIMP_NATIVE_TR); i++)
					if (strcmp(SHRIMP_NATIVE_TR[i].tag, tag) == 0)
						lang = i;
			}
			free(buf);
		}
	}
	return SHRIMP_NATIVE_TR[lang].s[id] ? SHRIMP_NATIVE_TR[lang].s[id] : SHRIMP_NATIVE_TR[0].s[id];
}

static void fatal(const wchar_t* msg)
{
	MessageBoxW(NULL, msg, L"Shrimp Install", MB_OK | MB_ICONERROR);
	PostQuitMessage(1);
}

static BOOL apps_dark(void)
{
	DWORD v = 1, size = sizeof(v);
	RegGetValueW(HKEY_CURRENT_USER, L"Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize",
		L"AppsUseLightTheme", RRF_RT_REG_DWORD, NULL, &v, &size);
	return v == 0;
}

/* ---------- obsługa zdarzeń WebView2 (obiekty COM w C) ---------- */
#define HANDLER(T, invoke_args) \
	static HRESULT STDMETHODCALLTYPE T##_qi(T* This, REFIID riid, void** ppv) { \
		if (IsEqualIID(riid, &IID_IUnknown) || IsEqualIID(riid, &IID_##T)) { *ppv = This; return S_OK; } \
		*ppv = NULL; return E_NOINTERFACE; } \
	static ULONG STDMETHODCALLTYPE T##_addref(T* This) { (void)This; return 1; } \
	static ULONG STDMETHODCALLTYPE T##_release(T* This) { (void)This; return 1; } \
	static HRESULT STDMETHODCALLTYPE T##_invoke invoke_args; \
	static T##Vtbl T##_vtbl = { T##_qi, T##_addref, T##_release, T##_invoke }; \
	static T T##_obj = { &T##_vtbl };

HANDLER(ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler,
	(ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler* This, HRESULT hr, ICoreWebView2Environment* result))
HANDLER(ICoreWebView2CreateCoreWebView2ControllerCompletedHandler,
	(ICoreWebView2CreateCoreWebView2ControllerCompletedHandler* This, HRESULT hr, ICoreWebView2Controller* result))
HANDLER(ICoreWebView2WebMessageReceivedEventHandler,
	(ICoreWebView2WebMessageReceivedEventHandler* This, ICoreWebView2* sender, ICoreWebView2WebMessageReceivedEventArgs* args))
HANDLER(ICoreWebView2NavigationStartingEventHandler,
	(ICoreWebView2NavigationStartingEventHandler* This, ICoreWebView2* sender, ICoreWebView2NavigationStartingEventArgs* args))
HANDLER(ICoreWebView2NewWindowRequestedEventHandler,
	(ICoreWebView2NewWindowRequestedEventHandler* This, ICoreWebView2* sender, ICoreWebView2NewWindowRequestedEventArgs* args))

static HRESULT STDMETHODCALLTYPE ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler_invoke(
	ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler* This, HRESULT hr, ICoreWebView2Environment* result)
{
	(void)This;
	if (FAILED(hr) || result == NULL) {
		uprintf("Shrimp: WebView2: środowisko 0x%08lX", hr);
		fatal(native_tr(SN_WV2MISSING));
		return S_OK;
	}
	env = result;
	ICoreWebView2Environment_AddRef(env);
	return ICoreWebView2Environment_CreateCoreWebView2Controller(env, hShrimpWnd, &ICoreWebView2CreateCoreWebView2ControllerCompletedHandler_obj);
}

static HRESULT STDMETHODCALLTYPE ICoreWebView2CreateCoreWebView2ControllerCompletedHandler_invoke(
	ICoreWebView2CreateCoreWebView2ControllerCompletedHandler* This, HRESULT hr, ICoreWebView2Controller* result)
{
	ICoreWebView2Settings* settings = NULL;
	ICoreWebView2Controller2* c2 = NULL;
	EventRegistrationToken tok;
	RECT rc;
	DWORD size;
	const BYTE* html;

	(void)This;
	if (FAILED(hr) || result == NULL) {
		uprintf("Shrimp: WebView2: kontroler 0x%08lX", hr);
		fatal(native_tr(SN_WINDOWFAIL));
		return S_OK;
	}
	controller = result;
	ICoreWebView2Controller_AddRef(controller);
	ICoreWebView2Controller_get_CoreWebView2(controller, &webview);

	/* Tło jak w interfejsie, żeby nie mignęła biel przed załadowaniem. */
	if (SUCCEEDED(ICoreWebView2Controller_QueryInterface(controller, &IID_ICoreWebView2Controller2, (void**)&c2))) {
		COREWEBVIEW2_COLOR bg = dark ? (COREWEBVIEW2_COLOR){ 255, 0x20, 0x20, 0x20 } : (COREWEBVIEW2_COLOR){ 255, 0xF3, 0xF3, 0xF3 };
		ICoreWebView2Controller2_put_DefaultBackgroundColor(c2, bg);
		ICoreWebView2Controller2_Release(c2);
	}
	if (SUCCEEDED(ICoreWebView2_get_Settings(webview, &settings))) {
		ICoreWebView2Settings3* s3 = NULL;
		ICoreWebView2Settings_put_AreDefaultContextMenusEnabled(settings, FALSE);
		ICoreWebView2Settings_put_AreDevToolsEnabled(settings, getenv("SHRIMP_DEVTOOLS") != NULL);
		ICoreWebView2Settings_put_IsStatusBarEnabled(settings, FALSE);
		ICoreWebView2Settings_put_IsZoomControlEnabled(settings, FALSE);
		ICoreWebView2Settings_put_AreDefaultScriptDialogsEnabled(settings, FALSE);
		ICoreWebView2Settings9* s9 = NULL;
		if (SUCCEEDED(ICoreWebView2Settings_QueryInterface(settings, &IID_ICoreWebView2Settings3, (void**)&s3))) {
			ICoreWebView2Settings3_put_AreBrowserAcceleratorKeysEnabled(s3, FALSE);
			ICoreWebView2Settings3_Release(s3);
		}
		/* CSS app-region: drag na pasku tytułu interfejsu */
		if (SUCCEEDED(ICoreWebView2Settings_QueryInterface(settings, &IID_ICoreWebView2Settings9, (void**)&s9))) {
			ICoreWebView2Settings9_put_IsNonClientRegionSupportEnabled(s9, TRUE);
			ICoreWebView2Settings9_Release(s9);
		}
		ICoreWebView2Settings_Release(settings);
	}
	GetClientRect(hShrimpWnd, &rc);
	ICoreWebView2Controller_put_Bounds(controller, rc);
	ICoreWebView2_add_WebMessageReceived(webview, &ICoreWebView2WebMessageReceivedEventHandler_obj, &tok);
	ICoreWebView2_add_NavigationStarting(webview, &ICoreWebView2NavigationStartingEventHandler_obj, &tok);
	ICoreWebView2_add_NewWindowRequested(webview, &ICoreWebView2NewWindowRequestedEventHandler_obj, &tok);
	if (opts_script != NULL)
		ICoreWebView2_AddScriptToExecuteOnDocumentCreated(webview, opts_script, NULL);

	html = resource(IDR_SHRIMP_UI, &size);
	wchar_t* page = html ? utf8_to_w((const char*)html, (int)size) : NULL;
	if (page == NULL) {
		fatal(native_tr(SN_NOUI));
		return S_OK;
	}
	ICoreWebView2_NavigateToString(webview, page);
	free(page);
	return S_OK;
}

static HRESULT STDMETHODCALLTYPE ICoreWebView2WebMessageReceivedEventHandler_invoke(
	ICoreWebView2WebMessageReceivedEventHandler* This, ICoreWebView2* sender, ICoreWebView2WebMessageReceivedEventArgs* args)
{
	LPWSTR m = NULL;
	(void)This; (void)sender;
	if (SUCCEEDED(ICoreWebView2WebMessageReceivedEventArgs_TryGetWebMessageAsString(args, &m)) && m != NULL) {
		char* u = wchar_to_utf8(m);
		if (u != NULL)
			ShrimpDispatch(u);
		free(u);
	}
	CoTaskMemFree(m);
	return S_OK;
}

/* Tylko pierwsza nawigacja (nasz interfejs); żadnych przejść do innych stron. */
static HRESULT STDMETHODCALLTYPE ICoreWebView2NavigationStartingEventHandler_invoke(
	ICoreWebView2NavigationStartingEventHandler* This, ICoreWebView2* sender, ICoreWebView2NavigationStartingEventArgs* args)
{
	static int count = 0;
	(void)This; (void)sender;
	if (count++ > 0)
		ICoreWebView2NavigationStartingEventArgs_put_Cancel(args, TRUE);
	return S_OK;
}

static HRESULT STDMETHODCALLTYPE ICoreWebView2NewWindowRequestedEventHandler_invoke(
	ICoreWebView2NewWindowRequestedEventHandler* This, ICoreWebView2* sender, ICoreWebView2NewWindowRequestedEventArgs* args)
{
	(void)This; (void)sender;
	ICoreWebView2NewWindowRequestedEventArgs_put_Handled(args, TRUE);
	return S_OK;
}

/* ---------- okno ---------- */
void ShrimpEmit(char* json)
{
	if (json == NULL)
		return;
	if (hShrimpWnd == NULL || !PostMessage(hShrimpWnd, WM_SHRIMP_EMIT, 0, (LPARAM)json))
		free(json);
}

static LRESULT CALLBACK WndProc(HWND hwnd, UINT msg, WPARAM wParam, LPARAM lParam)
{
	switch (msg) {
	case WM_NCCALCSIZE:
		/* Cały obszar okna należy do interfejsu (pasek tytułu jest w HTML), ramka i cień zostają. */
		if (wParam)
			return 0;
		break;
	case WM_NCHITTEST: {
		LRESULT r = DefWindowProcW(hwnd, msg, wParam, lParam);
		return (r == HTCAPTION || r == HTSYSMENU) ? HTCLIENT : r;
	}
	case WM_SIZE:
		if (controller != NULL) {
			RECT rc;
			GetClientRect(hwnd, &rc);
			ICoreWebView2Controller_put_Bounds(controller, rc);
		}
		return 0;
	case WM_DPICHANGED: {
		RECT* r = (RECT*)lParam;
		SetWindowPos(hwnd, NULL, r->left, r->top, r->right - r->left, r->bottom - r->top, SWP_NOZORDER | SWP_NOACTIVATE);
		return 0;
	}
	case WM_SETTINGCHANGE:
		if (lParam && wcscmp((const wchar_t*)lParam, L"ImmersiveColorSet") == 0) {
			BOOL d = apps_dark();
			if (d != dark) {
				dark = d;
				DwmSetWindowAttribute(hwnd, 20 /* DWMWA_USE_IMMERSIVE_DARK_MODE */, &dark, sizeof(dark));
			}
		}
		break;
	case WM_SHRIMP_EMIT: {
		char* json = (char*)lParam;
		ShrimpLogEvent(json);
		if (webview != NULL) {
			wchar_t* w = utf8_to_w(json, -1);
			if (w != NULL)
				ICoreWebView2_PostWebMessageAsJson(webview, w);
			free(w);
		}
		free(json);
		return 0;
	}
	case WM_SHRIMP_CALL:
	case WM_SHRIMP_SCANNED:
	case WM_SHRIMP_FORMATTED:
	case WM_SHRIMP_FETCHED:
	case WM_SHRIMP_COPIED:
	case WM_SHRIMP_BACKED_UP:
		return ShrimpWndMessage(msg, wParam, lParam);
	case WM_CLOSE:
		/* Alt+F4 w trakcie przygotowania: interfejs pyta, czy anulować. */
		if (!ShrimpCanClose()) {
			ShrimpEmit(_strdup("{\"event\":\"closeRequested\"}"));
			return 0;
		}
		DestroyWindow(hwnd);
		return 0;
	case WM_DESTROY:
		if (controller != NULL)
			ICoreWebView2Controller_Close(controller);
		hShrimpWnd = NULL;
		ShrimpWindowCleanup();
		/* Zamykamy też ukryte okno Rufusa (zapisuje ustawienia i kończy pętlę). */
		PostMessage(hMainDialog, WM_CLOSE, 0, 0);
		return 0;
	}
	return DefWindowProcW(hwnd, msg, wParam, lParam);
}

typedef HRESULT (STDAPICALLTYPE *create_env_t)(PCWSTR, PCWSTR, ICoreWebView2EnvironmentOptions*,
	ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler*);

/* WebView2Loader.dll z zasobów do %TEMP%\ShrimpInstall-<pid>. */
static create_env_t load_webview2(void)
{
	wchar_t tmp[MAX_PATH], dll[MAX_PATH];
	DWORD size, w;
	const BYTE* data = resource(IDR_SHRIMP_WV2LOADER, &size);
	HANDLE f;
	HMODULE h;

	if (data == NULL)
		return NULL;
	GetTempPathW(ARRAYSIZE(tmp), tmp);
	swprintf(tmp_dir, ARRAYSIZE(tmp_dir), L"%sShrimpInstall-%lu", tmp, GetCurrentProcessId());
	CreateDirectoryW(tmp_dir, NULL);
	swprintf(dll, ARRAYSIZE(dll), L"%s\\WebView2Loader.dll", tmp_dir);
	f = CreateFileW(dll, GENERIC_WRITE, 0, NULL, CREATE_ALWAYS, FILE_ATTRIBUTE_NORMAL, NULL);
	if (f == INVALID_HANDLE_VALUE)
		return NULL;
	BOOL ok = WriteFile(f, data, size, &w, NULL) && w == size;
	CloseHandle(f);
	if (!ok)
		return NULL;
	h = LoadLibraryExW(dll, NULL, LOAD_WITH_ALTERED_SEARCH_PATH);
	return h ? (create_env_t)(void*)GetProcAddress(h, "CreateCoreWebView2EnvironmentWithOptions") : NULL;
}

BOOL ShrimpWindowCreate(HINSTANCE hInstance, const char* opts_json)
{
	WNDCLASSEXW wc = { sizeof(wc) };
	wchar_t data_dir[MAX_PATH];
	MONITORINFO mi = { sizeof(mi) };
	UINT dpi;
	int w, h;
	HRESULT hr;

	CoInitializeEx(NULL, COINIT_APARTMENTTHREADED);
	dark = apps_dark();
	if (opts_json != NULL) {
		size_t n = strlen(opts_json) + 32;
		char* s = malloc(n);
		if (s != NULL) {
			snprintf(s, n, "window.SHRIMP_OPTS=%s;", opts_json);
			opts_script = utf8_to_w(s, -1);
			free(s);
		}
	}

	wc.lpfnWndProc = WndProc;
	wc.hInstance = hInstance;
	wc.hCursor = LoadCursor(NULL, IDC_ARROW);
	wc.hIcon = LoadIcon(hInstance, MAKEINTRESOURCE(IDI_ICON));
	wc.hbrBackground = CreateSolidBrush(dark ? RGB(0x20, 0x20, 0x20) : RGB(0xF3, 0xF3, 0xF3));
	wc.lpszClassName = cls_name;
	RegisterClassExW(&wc);

	/* Na środku ekranu z kursorem, 1000×700 w pikselach logicznych. */
	POINT pt;
	GetCursorPos(&pt);
	HMONITOR mon = MonitorFromPoint(pt, MONITOR_DEFAULTTOPRIMARY);
	GetMonitorInfoW(mon, &mi);
	{
		HDC dc = GetDC(NULL);
		dpi = GetDeviceCaps(dc, LOGPIXELSX);
		ReleaseDC(NULL, dc);
	}
	w = MulDiv(WIN_W, dpi, 96);
	h = MulDiv(WIN_H, dpi, 96);
	hShrimpWnd = CreateWindowExW(WS_EX_APPWINDOW, cls_name, L"Shrimp Install",
		WS_OVERLAPPED | WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX,
		mi.rcWork.left + ((mi.rcWork.right - mi.rcWork.left) - w) / 2,
		mi.rcWork.top + ((mi.rcWork.bottom - mi.rcWork.top) - h) / 2,
		w, h, NULL, NULL, hInstance, NULL);
	if (hShrimpWnd == NULL)
		return FALSE;
	{
		MARGINS m = { 0, 0, 1, 0 };
		DWORD round = 2; /* DWMWCP_ROUND */
		DwmExtendFrameIntoClientArea(hShrimpWnd, &m);
		DwmSetWindowAttribute(hShrimpWnd, 20 /* DWMWA_USE_IMMERSIVE_DARK_MODE */, &dark, sizeof(dark));
		DwmSetWindowAttribute(hShrimpWnd, 33 /* DWMWA_WINDOW_CORNER_PREFERENCE */, &round, sizeof(round));
		SetWindowPos(hShrimpWnd, NULL, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
	}
	ShowWindow(hShrimpWnd, SW_SHOWNORMAL);
	UpdateWindow(hShrimpWnd);

	create_env_t create = load_webview2();
	if (create == NULL) {
		fatal(native_tr(SN_WV2FAIL));
		return FALSE;
	}
	swprintf(data_dir, ARRAYSIZE(data_dir), L"%s\\WebView2", tmp_dir);
	hr = create(NULL, data_dir, NULL, &ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler_obj);
	if (FAILED(hr)) {
		ICoreWebView2CreateCoreWebView2EnvironmentCompletedHandler_invoke(NULL, hr, NULL);
		return FALSE;
	}
	return TRUE;
}

/* Sprzątanie folderu tymczasowego przy wyjściu (WebView2 może jeszcze trzymać pliki: najlepiej jak się da). */
void ShrimpWindowCleanup(void)
{
	wchar_t cmd[MAX_PATH + 64];
	STARTUPINFOW si = { sizeof(si) };
	PROCESS_INFORMATION pi;
	if (!tmp_dir[0])
		return;
	swprintf(cmd, ARRAYSIZE(cmd), L"cmd.exe /c ping -n 3 127.0.0.1 >nul & rmdir /s /q \"%s\"", tmp_dir);
	if (CreateProcessW(NULL, cmd, NULL, NULL, FALSE, CREATE_NO_WINDOW, NULL, NULL, &si, &pi)) {
		CloseHandle(pi.hThread);
		CloseHandle(pi.hProcess);
	}
}
