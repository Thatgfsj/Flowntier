/**
 * v0.4.36: unit tests for the EditorPane language-selection rule.
 *
 * The mapping is deliberately extracted into a pure function
 * (zones/editorUtil.ts) so the vitest run can assert it without
 * mounting CodeMirror. Coverage mirrors the v0.4.36 spec table:
 *   rs → rust
 *   js/jsx/ts/tsx → javascript
 *   json → json
 *   md/markdown → markdown
 *   py → python
 *   anything else → null (plain text, no language extension)
 */
import { describe, expect, it } from "vitest";
import { extOf, pickLanguageSupport } from "./editorUtil.js";

describe("pickLanguageSupport — extension → CodeMirror language", () => {
  it("maps .rs to rust", () => {
    expect(pickLanguageSupport("rs")).toBe("rust");
  });

  it("maps js, jsx, ts and tsx all to javascript", () => {
    expect(pickLanguageSupport("js")).toBe("javascript");
    expect(pickLanguageSupport("jsx")).toBe("javascript");
    expect(pickLanguageSupport("ts")).toBe("javascript");
    expect(pickLanguageSupport("tsx")).toBe("javascript");
  });

  it("maps json to json", () => {
    expect(pickLanguageSupport("json")).toBe("json");
  });

  it("maps md and markdown to markdown", () => {
    expect(pickLanguageSupport("md")).toBe("markdown");
    expect(pickLanguageSupport("markdown")).toBe("markdown");
  });

  it("maps py to python", () => {
    expect(pickLanguageSupport("py")).toBe("python");
  });

  it("returns null for unknown extensions (no language support)", () => {
    expect(pickLanguageSupport("toml")).toBeNull();
    expect(pickLanguageSupport("yaml")).toBeNull();
    expect(pickLanguageSupport("txt")).toBeNull();
    expect(pickLanguageSupport("")).toBeNull();
  });

  it("is case-insensitive (RS / TSX / JSON)", () => {
    expect(pickLanguageSupport("RS")).toBe("rust");
    expect(pickLanguageSupport("TSX")).toBe("javascript");
    expect(pickLanguageSupport("JSON")).toBe("json");
  });
});

describe("extOf — path → lowercase extension", () => {
  it("extracts the extension from posix paths", () => {
    expect(extOf("src/main.rs")).toBe("rs");
    expect(extOf("pkg/app.json")).toBe("json");
  });

  it("extracts the extension from windows paths", () => {
    expect(extOf("src\\main.rs")).toBe("rs");
  });

  it("lowercases the extension", () => {
    expect(extOf("README.MD")).toBe("md");
  });

  it("returns empty string for extensionless paths and dotfiles", () => {
    expect(extOf("Dockerfile")).toBe("");
    expect(extOf(".gitignore")).toBe("");
    expect(extOf("")).toBe("");
  });
});
