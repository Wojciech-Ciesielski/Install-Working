import { describe, expect, it } from "vitest";
import { helpUrl } from "../../../help-links/help";
import cfg from "../../../help-links/help.json";

describe("strony pomocy", () => {
  it("bez adresu z manifestu biorą adres z boxes/help-links/help.json (puste = sama podpowiedź)", () => {
    const want = cfg.base ? `${cfg.base.replace(/\/+$/, "")}/download-iso?lang=pl&from=install` : null;
    expect(helpUrl("iso", "pl", "install", "")).toBe(want);
  });
  it("adres strony to https://shrimp-organization.dev/help", () => {
    expect(cfg.base).toBe("https://shrimp-organization.dev/help");
  });
  it("składają adres z tematu, języka i programu", () => {
    expect(helpUrl("iso", "pl", "install", "https://example.org/help/")).toBe("https://example.org/help/download-iso?lang=pl&from=install");
    expect(helpUrl("nvidia", "pt-BR", "setup", "https://example.org")).toBe("https://example.org/nvidia-driver?lang=pt-BR&from=setup");
  });
  it("odrzucają nieznany temat i adresy inne niż https", () => {
    expect(helpUrl("nie-ma", "pl", "install", "https://example.org")).toBeNull();
    for (const b of ["http://example.org", "javascript:alert(1)", "https://exa mple.org", "https://a.org/?x=1", "file:///c:/"]) expect(helpUrl("iso", "pl", "install", b), b).toBeNull();
  });
});
