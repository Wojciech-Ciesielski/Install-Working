/*
 * Shrimp Install: jednorazowy start z nośnika, bez wchodzenia do ustawień UEFI.
 *
 * Tworzy wpis rozruchowy UEFI Boot#### „Shrimp Install” wskazujący \EFI\BOOT\BOOTX64.EFI na partycji
 * nośnika (krótka ścieżka HD(…)/File(…), którą firmware dopasowuje po identyfikatorze partycji)
 * i ustawia BootNext. Kolejność startu (BootOrder) się nie zmienia, więc kolejne uruchomienie
 * wraca do zwykłego porządku. To samo robi `bcdedit /set {fwbootmgr} bootsequence`.
 *
 * Copyright © 2026 Shrimp. GPLv3.
 */
#include <windows.h>
#include <winioctl.h>
#include <stdio.h>
#include <string.h>
#include <wchar.h>
#include "shrimp.h"
#include "../rufus.h"
#include "../msapi_utf8.h"

#define EFI_GLOBAL L"{8BE4DF61-93CA-11D2-AA0D-00E098032B8C}"
#define LOAD_OPTION_ACTIVE 0x00000001
static const wchar_t description[] = L"Shrimp Install";
static const wchar_t loader[] = L"\\EFI\\BOOT\\BOOTX64.EFI";

static BOOL privilege(LPCTSTR name)
{
	HANDLE tok;
	TOKEN_PRIVILEGES tp = { 1 };
	BOOL r;
	if (!OpenProcessToken(GetCurrentProcess(), TOKEN_ADJUST_PRIVILEGES | TOKEN_QUERY, &tok))
		return FALSE;
	r = LookupPrivilegeValue(NULL, name, &tp.Privileges[0].Luid);
	tp.Privileges[0].Attributes = SE_PRIVILEGE_ENABLED;
	r = r && AdjustTokenPrivileges(tok, FALSE, &tp, 0, NULL, NULL) && GetLastError() == ERROR_SUCCESS;
	CloseHandle(tok);
	return r;
}

#pragma pack(push, 1)
typedef struct {
	uint8_t type, subtype;
	uint16_t length;
	uint32_t number;
	uint64_t start, size;
	uint8_t signature[16];
	uint8_t format, signature_type;
} hd_node;
#pragma pack(pop)

/* Partycja z systemem plików nośnika (ta, którą Rufus utworzył jako pierwszą z danymi). */
static BOOL medium_partition(DWORD DriveIndex, hd_node* hd)
{
	char path[64];
	BYTE layout_buf[sizeof(DRIVE_LAYOUT_INFORMATION_EX) + 16 * sizeof(PARTITION_INFORMATION_EX)];
	DRIVE_LAYOUT_INFORMATION_EX* layout = (DRIVE_LAYOUT_INFORMATION_EX*)layout_buf;
	DISK_GEOMETRY_EX geo;
	DWORD n;
	BOOL ok = FALSE;

	static_sprintf(path, "\\\\.\\PhysicalDrive%lu", DriveIndex - DRIVE_INDEX_MIN);
	HANDLE h = CreateFileA(path, GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE, NULL, OPEN_EXISTING, 0, NULL);
	if (h == INVALID_HANDLE_VALUE)
		return FALSE;
	if (!DeviceIoControl(h, IOCTL_DISK_GET_DRIVE_GEOMETRY_EX, NULL, 0, &geo, sizeof(geo), &n, NULL)
		|| !DeviceIoControl(h, IOCTL_DISK_GET_DRIVE_LAYOUT_EX, NULL, 0, layout, sizeof(layout_buf), &n, NULL))
		goto out;
	DWORD sector = geo.Geometry.BytesPerSector ? geo.Geometry.BytesPerSector : 512;
	for (DWORD i = 0; i < layout->PartitionCount; i++) {
		PARTITION_INFORMATION_EX* p = &layout->PartitionEntry[i];
		if (p->PartitionLength.QuadPart == 0 || p->PartitionNumber == 0)
			continue;
		memset(hd, 0, sizeof(*hd));
		hd->type = 4;          /* Media Device Path */
		hd->subtype = 1;       /* Hard Drive */
		hd->length = sizeof(*hd);
		hd->number = p->PartitionNumber;
		hd->start = p->StartingOffset.QuadPart / sector;
		hd->size = p->PartitionLength.QuadPart / sector;
		if (layout->PartitionStyle == PARTITION_STYLE_GPT) {
			memcpy(hd->signature, &p->Gpt.PartitionId, 16);
			hd->format = 2;
			hd->signature_type = 2;
		} else if (layout->PartitionStyle == PARTITION_STYLE_MBR) {
			memcpy(hd->signature, &layout->Mbr.Signature, 4);
			hd->format = 1;
			hd->signature_type = 1;
		} else {
			continue;
		}
		ok = TRUE;
		break;
	}
out:
	CloseHandle(h);
	return ok;
}

/* Wolny numer Boot#### albo numer istniejącego wpisu „Shrimp Install”. */
static int boot_number(void)
{
	BYTE buf[4096];
	wchar_t name[16];
	int free_num = -1;
	for (int i = 0; i < 0x1000; i++) {
		swprintf(name, ARRAYSIZE(name), L"Boot%04X", i);
		DWORD n = GetFirmwareEnvironmentVariableW(name, EFI_GLOBAL, buf, sizeof(buf));
		if (n == 0) {
			if (free_num < 0)
				free_num = i;
			if (i > 0x40 && free_num >= 0)
				break;
			continue;
		}
		if (n > 6 + sizeof(description) && memcmp(buf + 6, description, sizeof(description)) == 0)
			return i;
	}
	return free_num;
}

const char* ShrimpSetBootNext(DWORD DriveIndex)
{
	FIRMWARE_TYPE fw = FirmwareTypeUnknown;
	hd_node hd;
	BYTE opt[512];
	size_t off = 0;
	wchar_t name[16];
	uint16_t file_len = (uint16_t)(4 + sizeof(loader)), list_len;
	int num;

	GetFirmwareType(&fw);
	if (fw != FirmwareTypeUefi)
		return "noUefi";
	if (!privilege(SE_SYSTEM_ENVIRONMENT_NAME))
		return "noPriv";
	if (!medium_partition(DriveIndex, &hd))
		return "noMedia";
	num = boot_number();
	if (num < 0)
		return "noSlot";

	/* EFI_LOAD_OPTION: atrybuty, długość listy ścieżek, opis, ścieżka: HD(…)/File(…)/End */
	list_len = (uint16_t)(sizeof(hd) + file_len + 4);
	*(uint32_t*)&opt[off] = LOAD_OPTION_ACTIVE; off += 4;
	*(uint16_t*)&opt[off] = list_len; off += 2;
	memcpy(&opt[off], description, sizeof(description)); off += sizeof(description);
	memcpy(&opt[off], &hd, sizeof(hd)); off += sizeof(hd);
	opt[off++] = 4;            /* Media */
	opt[off++] = 4;            /* File Path */
	*(uint16_t*)&opt[off] = file_len; off += 2;
	memcpy(&opt[off], loader, sizeof(loader)); off += sizeof(loader);
	opt[off++] = 0x7F;         /* End of Hardware Device Path */
	opt[off++] = 0xFF;
	*(uint16_t*)&opt[off] = 4; off += 2;

	swprintf(name, ARRAYSIZE(name), L"Boot%04X", num);
	if (!SetFirmwareEnvironmentVariableW(name, EFI_GLOBAL, opt, (DWORD)off)) {
		uprintf("Shrimp: zapis %S nie powiódł się: %s", name, WindowsErrorString());
		return "fwEntry";
	}
	uint16_t next = (uint16_t)num;
	if (!SetFirmwareEnvironmentVariableW(L"BootNext", EFI_GLOBAL, &next, sizeof(next))) {
		uprintf("Shrimp: zapis BootNext nie powiódł się: %s", WindowsErrorString());
		return "fwNext";
	}
	uprintf("Shrimp: BootNext = %S (partycja %lu nośnika)", name, hd.number);
	return NULL;
}

/* Szybkie uruchamianie zostawia NTFS w stanie hibernacji, a wtedy Setup nie zmniejszy partycji Windows. */
void ShrimpDisableFastStartup(void)
{
	HKEY k;
	DWORD zero = 0;
	if (RegOpenKeyExW(HKEY_LOCAL_MACHINE, L"SYSTEM\\CurrentControlSet\\Control\\Session Manager\\Power", 0, KEY_SET_VALUE, &k) == ERROR_SUCCESS) {
		RegSetValueExW(k, L"HiberbootEnabled", 0, REG_DWORD, (const BYTE*)&zero, sizeof(zero));
		RegCloseKey(k);
	}
}

BOOL ShrimpReboot(void)
{
	privilege(SE_SHUTDOWN_NAME);
	return ExitWindowsEx(EWX_REBOOT, SHTDN_REASON_MAJOR_OPERATINGSYSTEM | SHTDN_REASON_MINOR_INSTALLATION | SHTDN_REASON_FLAG_PLANNED);
}
