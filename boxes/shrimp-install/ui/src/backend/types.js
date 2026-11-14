// Umowa między interfejsem Shrimp Center a programem (Rust, src/win/app.rs i src/win/systems.rs).
// Plik tylko z opisem (bez kodu): rozmiary w bajtach, czasy w sekundach.
//
// Device   { id: numer dysku Windows, name: „SanDisk Ultra (E:)”, kind: usb | sd | disk, size }
// Folder   { key: documents | pictures | desktop | music | videos | downloads, path, size: liczba albo null (liczy się) }
// Picked   { path, name, kind: folder | file, size }
// Info     { bitlocker, shrimpSize (z manifestu albo szacunek), minSize, folders: Folder[], helpUrl? (https://…) }
// Progress { phase: 1 | 2 | 3, done, total, speed (B/s, 0 = nie wiadomo), step?: verify | prepare | backup }
// Failure  { phase, kind: offline | verify | write | copy | space | device | busy | question | other, message? }
// Plan     { device, folders: klucze (tryb „Moje foldery”), paths: ścieżki (tryb „Wybiorę sam”) }
// Systems  { firmware: uefi | bios | unknown, shrimp: { id: „0002”, name } | null, default: windows | shrimp | null }
//
// Backend:
//   info(): Promise<Info>
//   onDevices(cb(Device[])), onFolders(cb(Folder[])), onBitLocker(cb(on)), onCloseRequest(cb())
//   pick(kind): Promise<Picked[]>
//   start(plan, { progress(p), image?(size), fail(f), done() }), cancel(): Promise
//   reboot(): Promise                     jednorazowy start z nośnika i restart (błąd: kod dla hostMessage)
//   systems(): Promise<Systems>           zakładki Systemy i Odinstaluj Shrimp
//   bootShrimp(): Promise                 BootNext na Shrimp i restart
//   setDefault("windows" | "shrimp"): Promise
//   firmwareReboot(): Promise             restart do ustawień UEFI
//   uninstall(): Promise<{ removed }>     wpisy UEFI Shrimp i pliki z partycji EFI
//   openDisks()                           Zarządzanie dyskami
//   openUrl(https://…), later(), minimize(), close(), drag()
