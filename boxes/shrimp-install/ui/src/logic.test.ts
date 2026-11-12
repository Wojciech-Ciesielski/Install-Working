import { describe, expect, it } from "vitest";
import { GiB, dur, fmtGB, fmtSize, hostMessage, minLabel, phaseFrac, phaseText, remaining, space } from "./logic";
import { load } from "./i18n";

const en = load(["en-US"]);
const pl = load(["pl-PL"]);
const n = (s: string) => s.replace(/[  ]/g, " ");

describe("formatowanie", () => {
  it("rozmiary nośników jak w Eksploratorze", () => {
    expect(n(fmtGB(pl, 32e9))).toBe("29,8 GB");
    expect(n(fmtGB(en, 32e9))).toBe("29.8 GB");
    expect(n(fmtGB(pl, 500e9))).toBe("466 GB");
    expect(n(fmtGB(pl, 1e12))).toBe("931 GB");
    expect(n(fmtGB(pl, 2e12))).toBe("1,8 TB");
    expect(n(fmtGB(pl, 1024 * GiB))).toBe("1 TB");
  });
  it("małe pliki w MB", () => {
    expect(n(fmtSize(pl, 2 * 1024 ** 2))).toBe("2 MB");
    expect(n(fmtSize(pl, 100))).toBe("1 MB");
    expect(n(fmtSize(pl, 3.2 * GiB))).toBe("3,2 GB");
  });
  it("czas", () => {
    expect(dur(pl, 30)).toBe("mniej niż minuta");
    expect(dur(en, 70)).toBe("about 1 minute");
    expect(dur(pl, 720)).toBe("około 12 minut");
    expect(dur(en, 720)).toBe("about 12 minutes");
  });
  it("wymagana pojemność jak na opakowaniu", () => {
    expect(n(minLabel(pl, 14e9))).toBe("16 GB");
    expect(n(minLabel(en, 7e9))).toBe("8 GB");
  });
  it("jednostki w języku interfejsu", () => {
    expect(n(fmtGB(load(["ru"]), 32e9))).toBe("29,8 ГБ");
  });
});

describe("miejsce na nośniku", () => {
  it("liczy wolne i brakujące miejsce", () => {
    const s = space(pl, 29.8 * GiB, 3.1 * GiB, 19.6 * GiB);
    expect(s.over).toBe(false);
    expect(n(s.text)).toBe("Wolne 7,1 GB z 29,8 GB");
    const o = space(pl, 14.4 * GiB, 3.1 * GiB, 12 * GiB);
    expect(o.over).toBe(true);
    expect(n(o.text)).toBe("Brakuje 0,7 GB");
    expect(n(space(en, 14.4 * GiB, 3.1 * GiB, 12 * GiB).text)).toBe("0.7 GB short");
    expect(o.filesPct).toBeCloseTo(((14.4 - 3.1) / 14.4) * 100);
  });
});

describe("postęp", () => {
  const sizes = { shrimp: 3.1 * GiB, files: 19.6 * GiB };
  it("opisy faz jak w makiecie", () => {
    const p = { phase: 1 as const, done: 1.2 * GiB, total: 3.1 * GiB, speed: 14 * 1024 ** 2 };
    expect(n(phaseText(pl, 1, p, sizes, false))).toBe("1,2 z 3,1 GB · 14 MB/s · zostało około 2 minuty");
    expect(n(phaseText(pl, 2, p, sizes, false))).toBe("Zajmie około 1 minuta");
    expect(n(phaseText(pl, 3, p, sizes, false))).toBe("19,6 GB · zajmie około 6 minut");
    expect(n(phaseText(pl, 1, null, sizes, true))).toBe("Pobrano 3,1 GB");
    expect(n(phaseText(pl, 3, null, sizes, true))).toBe("Skopiowano 19,6 GB");
    expect(phaseText(pl, 3, null, { ...sizes, files: 0 }, false)).toBe("Nic do skopiowania");
    expect(n(phaseText(en, 1, p, sizes, false))).toBe("1.2 of 3.1 GB · 14 MB/s · about 2 minutes left");
  });
  it("paski i pozostały czas", () => {
    const p = { phase: 2 as const, done: 1, total: 4, speed: 0 };
    expect(phaseFrac(1, p, false)).toBe(1);
    expect(phaseFrac(2, p, false)).toBe(0.25);
    expect(phaseFrac(3, p, false)).toBe(0);
    expect(remaining({ phase: 3, done: 0, total: 40 * 1024 ** 2, speed: 40 * 1024 ** 2 }, sizes)).toBeCloseTo(1);
  });
});

describe("kroki w fazach (Beta 2: mylące „pobieranie 1116 MB/s” i „0%” przy zapisie)", () => {
  const sizes = { shrimp: 1.94 * GiB, files: 0 };
  it("sprawdzanie pobranego wcześniej obrazu nie udaje pobierania", () => {
    const p = { phase: 1 as const, done: 0.97 * GiB, total: 1.94 * GiB, speed: 1116 * 1024 ** 2, step: "verify" as const };
    expect(n(phaseText(pl, 1, p, sizes, false))).toBe("Sprawdzanie pobranego wcześniej obrazu · 50%");
  });
  it("przygotowanie i kopia zawartości nośnika mają własny opis", () => {
    expect(phaseText(pl, 2, { phase: 2, done: 0, total: 0, speed: 0, step: "prepare" }, sizes, false)).toBe("Przygotowanie nośnika…");
    const b = { phase: 2 as const, done: 1 * GiB, total: 4 * GiB, speed: 50 * 1024 ** 2, step: "backup" as const };
    expect(n(phaseText(pl, 2, b, sizes, false))).toBe("Kopia zawartości nośnika · 25% · zostało około 1 minuta");
    expect(remaining(b, sizes)).toBeGreaterThan(sizes.shrimp / (38 * 1024 ** 2));
  });
  it("po zakończeniu pobierania podaje prawdziwy rozmiar obrazu", () => {
    expect(n(phaseText(pl, 1, { phase: 2, done: 0, total: 0, speed: 0 }, sizes, false))).toBe("Pobrano 1,9 GB");
  });
});

describe("komunikaty programu", () => {
  it("kody tłumaczy, nieznany tekst zostawia", () => {
    expect(hostMessage(en, "connect")).toBe("Can't connect to the download server.");
    expect(hostMessage(pl, "http:404")).toBe("Serwer pobierania odpowiedział kodem 404.");
    expect(hostMessage(en, "Błąd 5: odmowa dostępu")).toBe("Błąd 5: odmowa dostępu");
  });
});
