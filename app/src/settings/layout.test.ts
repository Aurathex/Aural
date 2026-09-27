// Guards the settings window's single scroll area. jsdom has no layout engine, so these
// check the CSS rules that make it work; the real window is checked by eye at 720x520,
// 820x580 and 1280x800 (docs/release/v0.3-readiness.md).
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

const read = (p: string) => readFileSync(new URL(p, import.meta.url), "utf8");
const rule = (css: string, selector: string) => {
  const esc = selector.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const m = css.match(new RegExp(`(?:^|[}\\s])${esc}\\s*\\{([^}]*)\\}`, "m"));
  return m ? m[1] : "";
};

describe("About page branding", () => {
  const about = read("./About.svelte");

  it("names Aurathex as the maker next to the Aural mark", () => {
    expect(about).toMatch(/class="maker"[^>]*>by Aurathex</);
    expect(about).toMatch(/Made by Aurathex/);
    expect(about).toMatch(/© \{year\} Aurathex/);
  });
});

describe("settings window scrolling", () => {
  const app = read("./App.svelte");
  const ui = read("./ui.css");

  it("the page itself never scrolls: only the content pane does", () => {
    expect(rule(ui, "html, body")).toMatch(/overflow:\s*hidden/);
    expect(rule(ui, "html, body")).toMatch(/overscroll-behavior:\s*none/);
    expect(rule(app, ".shell")).toMatch(/overflow:\s*hidden/);
  });

  it("the content pane contains its own absolutely positioned helpers", () => {
    // Screen-reader-only spans are position:absolute; without a positioned ancestor
    // they escaped to the document and made it scroll into blank space.
    const main = rule(app, "main");
    expect(main).toMatch(/position:\s*relative/);
    expect(main).toMatch(/overflow-y:\s*auto/);
    expect(main).toMatch(/overscroll-behavior:\s*contain/);
  });

  it("chips that hold screen-reader text are their own containing block", () => {
    const row = read("../lib/ModelRow.svelte");
    expect(rule(row, ".chip")).toMatch(/position:\s*relative/);
  });
});
