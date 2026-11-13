//! Nośniki i obraz przez moduł Storage Windows.
//!
//! Układ nośnika Shrimp (GPT, start z UEFI):
//!   1. FAT32 „SHRIMP_BOOT” (typ: dane podstawowe, ≥ 1 GiB): EFI\BOOT, boot\ (GRUB, jądro, initrd) — wszystko z obrazu poza LiveOS
//!   2. exFAT „SHRIMP_LIVE” (reszta nośnika): LiveOS\squashfs.img, shrimp-transfer\, shrimp-backup\
//!
//! System live szuka nośnika po etykiecie (root=live:CDLABEL=SHRIMP_LIVE) i montuje partycję exFAT
//! w /run/overlay/live; pliki > 4 GB (obraz, filmy) mieszczą się bez dzielenia.
use super::ps::{self, q};
use crate::backup::Volume;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

pub const BASIC_DATA: &str = "{ebd0a0a2-b9e5-4433-87c0-68b6b72699c7}";

#[derive(Debug, Clone, Deserialize, PartialEq)]
pub struct Disk {
    pub id: u32,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub bus: String,
    pub size: u64,
    #[serde(default)]
    pub serial: Option<String>,
    #[serde(default)]
    pub uid: Option<String>,
    #[serde(default)]
    pub letters: Vec<String>,
    #[serde(default)]
    pub style: String,
}

impl Disk {
    /// „SanDisk Ultra USB Device” → „SanDisk Ultra (E:)”.
    pub fn label(&self) -> String {
        let mut n = self.name.clone().unwrap_or_default().replace(" USB Device", "").trim().to_string();
        if n.is_empty() {
            n = format!("Dysk {}", self.id);
        }
        match self.letters.first() {
            Some(l) if !l.is_empty() => format!("{n} ({l}:)"),
            _ => n,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self.bus.as_str() {
            "SD" | "MMC" => "sd",
            _ if self.size > 600_000_000_000 => "disk",
            _ => "usb",
        }
    }

    pub fn to_json(&self) -> Value {
        json!({ "id": self.id, "name": self.label(), "kind": self.kind(), "size": self.size })
    }

    /// Stały identyfikator nośnika (folder kopii zawartości).
    pub fn ident(&self) -> String {
        format!("{}|{}|{}|{}", self.name.as_deref().unwrap_or(""), self.serial.as_deref().unwrap_or(""), self.uid.as_deref().unwrap_or(""), self.size)
    }
}

const DISK_FIELDS: &str = "[pscustomobject]@{ id=[int]$_.Number; name=[string]$_.FriendlyName; bus=[string]$_.BusType; size=[uint64]$_.Size; serial=[string]$_.SerialNumber; uid=[string]$_.UniqueId; style=[string]$_.PartitionStyle; letters=@(Get-Partition -DiskNumber $_.Number -ErrorAction SilentlyContinue | Where-Object { $_.DriveLetter } | ForEach-Object { [string]$_.DriveLetter }) }";

/// Pendrive'y, karty SD i dyski USB, bez dysku z Windows. `also`: dysk testowy (--auto-disk), niezależnie od magistrali.
pub fn list(also: Option<u32>) -> Result<Vec<Disk>, String> {
    let extra = also.map_or(String::from("-1"), |n| n.to_string());
    ps::json(&format!(
        "$r = @(Get-Disk | Where-Object {{ -not $_.IsBoot -and -not $_.IsSystem -and $_.Size -gt 0 -and (@('USB','SD','MMC') -contains [string]$_.BusType -or $_.Number -eq {extra}) }} | Sort-Object Number | ForEach-Object {{ {DISK_FIELDS} }})\nConvertTo-Json -InputObject $r -Compress -Depth 3"
    ))
}

pub fn find(n: u32) -> Result<Disk, String> {
    ps::json(&format!("Get-Disk -Number {n} | ForEach-Object {{ {DISK_FIELDS} }} | ConvertTo-Json -Compress -Depth 3"))
}

#[derive(Deserialize)]
struct RawVol {
    path: String,
    label: String,
    fs: String,
    locked: bool,
}

/// Woluminy nośnika przed formatowaniem. Zablokowany (BitLocker To Go) albo nieczytelny wolumin to błąd:
/// nie formatujemy czegoś, czego nie umiemy skopiować.
pub fn volumes(n: u32) -> Result<Vec<Volume>, String> {
    let raw: Vec<RawVol> = ps::json(&format!(
        "$r = @(Get-Partition -DiskNumber {n} -ErrorAction SilentlyContinue | Where-Object {{ $_.Type -ne 'Reserved' -and $_.Type -ne 'Unknown' }} | ForEach-Object {{
  $v = $_ | Get-Volume -ErrorAction SilentlyContinue
  if ($v -and $v.Path) {{
    $locked = $false
    if (-not $v.FileSystem -and $v.DriveLetter) {{ try {{ $locked = (Get-BitLockerVolume -MountPoint \"$($v.DriveLetter):\").LockStatus -eq 'Locked' }} catch {{}} }}
    [pscustomobject]@{{ path=[string]$v.Path; label=[string]$v.FileSystemLabel; fs=[string]$v.FileSystem; locked=$locked }}
  }}
}})
ConvertTo-Json -InputObject $r -Compress"
    ))?;
    if raw.iter().any(|v| v.locked) {
        return Err("locked".into());
    }
    Ok(raw.into_iter().filter(|v| !v.fs.is_empty()).map(|v| Volume { root: PathBuf::from(v.path), label: v.label, fs: v.fs }).collect())
}

#[derive(Deserialize)]
struct Prepared {
    boot: String,
    live: String,
}

/// Czyści nośnik i tworzy układ Shrimp. Zwraca katalogi główne (ścieżki woluminów) partycji FAT32 i exFAT.
pub fn prepare(n: u32, boot_bytes: u64) -> Result<(PathBuf, PathBuf), String> {
    let p: Prepared = ps::json(&format!(
        "$n = {n}
$d = Get-Disk -Number $n
if ($d.IsBoot -or $d.IsSystem) {{ throw 'SYSTEM_DISK' }}
if ($d.IsOffline) {{ Set-Disk -Number $n -IsOffline $false }}
if ($d.IsReadOnly) {{ Set-Disk -Number $n -IsReadOnly $false }}
if ([string]$d.PartitionStyle -ne 'RAW') {{ Clear-Disk -Number $n -RemoveData -RemoveOEM -Confirm:$false }}
# Pendrive'y (nośniki wymienne) po Clear-Disk często zostają zainicjowane (MBR), a Initialize-Disk kończy się
# wtedy błędem „The disk has already been initialized”. Styl partycji czytamy więc od nowa i zmieniamy go na GPT.
Update-HostStorageCache -ErrorAction SilentlyContinue
$d = Get-Disk -Number $n
Get-Partition -DiskNumber $n -ErrorAction SilentlyContinue | Remove-Partition -Confirm:$false -ErrorAction SilentlyContinue
switch ([string]$d.PartitionStyle) {{
  'RAW' {{ Initialize-Disk -Number $n -PartitionStyle GPT }}
  'GPT' {{ }}
  default {{ Set-Disk -Number $n -PartitionStyle GPT }}
}}
Get-Partition -DiskNumber $n -ErrorAction SilentlyContinue | Where-Object {{ $_.Type -eq 'Reserved' }} | Remove-Partition -Confirm:$false
$b = New-Partition -DiskNumber $n -Size {boot_bytes} -GptType '{BASIC_DATA}'
$bv = Format-Volume -Partition $b -FileSystem FAT32 -NewFileSystemLabel 'SHRIMP_BOOT' -Confirm:$false -Force
$l = New-Partition -DiskNumber $n -UseMaximumSize -GptType '{BASIC_DATA}'
$lv = Format-Volume -Partition $l -FileSystem exFAT -NewFileSystemLabel 'SHRIMP_LIVE' -Confirm:$false -Force
[pscustomobject]@{{ boot=[string]$bv.Path; live=[string]$lv.Path }} | ConvertTo-Json -Compress"
    ))?;
    if p.boot.is_empty() || p.live.is_empty() {
        return Err("brak ścieżki woluminu po formatowaniu".into());
    }
    Ok((PathBuf::from(p.boot), PathBuf::from(p.live)))
}

/// Montuje obraz ISO (tylko do odczytu) i zwraca ścieżkę jego woluminu.
pub fn mount_iso(path: &str) -> Result<PathBuf, String> {
    let s = ps::run(&format!(
        "$p = {}
$img = Get-DiskImage -ImagePath $p
if (-not $img.Attached) {{ $null = Mount-DiskImage -ImagePath $p -StorageType ISO -Access ReadOnly }}
$v = $null
for ($i = 0; $i -lt 40 -and -not $v; $i++) {{ $v = Get-DiskImage -ImagePath $p | Get-Volume -ErrorAction SilentlyContinue; if (-not $v) {{ Start-Sleep -Milliseconds 250 }} }}
if (-not $v) {{ throw 'brak woluminu obrazu' }}
[string]$v.Path",
        q(path)
    ))?;
    Ok(PathBuf::from(s.lines().last().unwrap_or("").trim()))
}

pub fn dismount_iso(path: &str) {
    if let Err(e) = ps::run(&format!("Dismount-DiskImage -ImagePath {} | Out-Null", q(path))) {
        crate::log!("odmontowanie obrazu: {e}");
    }
}

#[derive(Debug, Deserialize)]
pub struct BootPart {
    pub number: u32,
    pub offset: u64,
    pub size: u64,
    pub guid: String,
    pub sector: u32,
    pub style: String,
    #[serde(default)]
    pub signature: Option<u32>,
}

/// Pierwsza partycja nośnika z danymi (FAT32 z EFI\BOOT\BOOTX64.EFI), dla wpisu rozruchowego UEFI.
pub fn boot_partition(n: u32) -> Result<BootPart, String> {
    ps::json(&format!(
        "$d = Get-Disk -Number {n}
$p = Get-Partition -DiskNumber {n} | Where-Object {{ $_.Type -ne 'Reserved' }} | Sort-Object Offset | Select-Object -First 1
if (-not $p) {{ throw 'brak partycji' }}
[pscustomobject]@{{ number=[int]$p.PartitionNumber; offset=[uint64]$p.Offset; size=[uint64]$p.Size; guid=[string]$p.Guid; sector=[int]$d.LogicalSectorSize; style=[string]$d.PartitionStyle; signature=$d.Signature }} | ConvertTo-Json -Compress"
    ))
}

/// Dysk systemowy z BitLockerem (instalacja obok Windows niemożliwa).
pub fn bitlocker_on() -> bool {
    ps::run("try { (Get-BitLockerVolume -MountPoint $env:SystemDrive).ProtectionStatus } catch { 'Unknown' }").is_ok_and(|s| s.trim() == "On")
}
