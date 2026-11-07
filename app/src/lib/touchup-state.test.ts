import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { latestTouchup } from "./touchup-state";
import type { TouchupPreview } from "./types";

const preview = (id: number, phase: TouchupPreview["phase"]): TouchupPreview => ({
  id, phase, original: "Original", target: "Scratch — Notepad", suggestion: "Revision", error: null,
});

describe("touch-up snapshot ordering", () => {
  it.each(["done", "error"] as const)("keeps %s when a delayed applying snapshot arrives", (phase) => {
    const completed = preview(3, phase);
    expect(latestTouchup(completed, preview(3, "applying"))).toBe(completed);
  });
  it("keeps every phase transition monotonic within the same session", () => {
    const phases = ["", "working", "review", "applying", "done"] as const;
    for (let i = 1; i < phases.length; i++) {
      const current = preview(3, phases[i]!);
      for (let j = 0; j < i; j++) expect(latestTouchup(current, preview(3, phases[j]!))).toBe(current);
      expect(latestTouchup(preview(3, phases[i - 1]!), current)).toBe(current);
    }
  });
  it("accepts a newer session even when its phase restarts", () => {
    const next = preview(4, "working");
    expect(latestTouchup(preview(3, "done"), next)).toBe(next);
  });
  it("keeps cancellation's higher id when an old result arrives", () => {
    const cancelled = preview(4, "");
    expect(latestTouchup(preview(3, "review"), cancelled)).toBe(cancelled);
    expect(latestTouchup(cancelled, preview(3, "done"))).toBe(cancelled);
    const next = preview(5, "working");
    expect(latestTouchup(cancelled, next)).toBe(next);
  });
  it("accepts initial and equal-phase updated payloads", () => {
    const incoming = preview(3, "review");
    expect(latestTouchup(undefined, incoming)).toBe(incoming);
    expect(latestTouchup(preview(3, "review"), incoming)).toBe(incoming);
  });
});


it("a delayed Cancel completion cannot hide a new reactive preview", async () => {
  // Exercise the actual handler with a reactive preview getter; no DOM is needed.
  const component = readFileSync(new URL("../settings/Touchup.svelte", import.meta.url), "utf8");
  const body = component.match(/async function close\(\) \{([\s\S]*?)\n  \}/)?.[1];
  expect(body).toBeDefined();
  let current = preview(3, "review");
  let complete!: () => void;
  const cancelled = new Promise<void>((resolve) => { complete = resolve; });
  const sentIds: number[] = [];
  const state = { get preview() { return current; }, busy: false, error: null };
  const call = async (_cmd: string, args: { id: number }) => { sentIds.push(args.id); await cancelled; };
  const update = (incoming: TouchupPreview) => { current = latestTouchup(current, incoming); };
  const close = new Function("state", "call", "update", `return async function () { with (state) { ${body} } };`)(state, call, update) as () => Promise<void>;
  const closing = close();
  expect(sentIds).toEqual([3]);
  // The backend has cancelled 3 and the next shortcut started session 4.
  const newer = preview(4, "working");
  current = newer;
  complete();
  await closing;
  expect(current).toBe(newer);
  const reviewed = preview(4, "review");
  update(reviewed);
  expect(current).toBe(reviewed);
});


it("an old Apply rejection cannot turn a new session into an error", async () => {
  const component = readFileSync(new URL("../settings/Touchup.svelte", import.meta.url), "utf8");
  const body = component.match(/async function apply\(\) \{([\s\S]*?)\n  \}/)?.[1]?.replace("call<TouchupPreview>", "call");
  expect(body).toBeDefined();
  let current = preview(3, "review");
  let reject!: (error: Error) => void;
  const pending = new Promise<never>((_resolve, fail) => { reject = fail; });
  const sentIds: number[] = [];
  const state = { get preview() { return current; }, error: null };
  const call = async (_cmd: string, args: { id: number }) => { sentIds.push(args.id); return pending; };
  const update = (incoming: TouchupPreview) => { current = latestTouchup(current, incoming); };
  const apply = new Function("state", "call", "update", `return async function () { with (state) { ${body} } };`)(state, call, update) as () => Promise<void>;
  const applying = apply();
  expect(sentIds).toEqual([3]);
  expect(current.phase).toBe("applying");
  const newer = preview(4, "working");
  current = newer;
  reject(new Error("late transport failure"));
  await applying;
  expect(current).toBe(newer);
  expect(state.error).toBeNull();
});
