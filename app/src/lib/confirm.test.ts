import { describe, expect, it } from "vitest";
import { DELETE_PHRASE, isDeleteConfirmed } from "./confirm";

describe("delete confirmation", () => {
  it("accepts only the exact phrase", () => {
    expect(DELETE_PHRASE).toBe("YES, DELETE");
    expect(isDeleteConfirmed("YES, DELETE")).toBe(true);
  });

  it.each([
    "",
    "yes, delete",
    "YES DELETE",
    "YES,DELETE",
    " YES, DELETE",
    "YES, DELETE ",
    "YES, DELETE.",
    "Yes, Delete",
    "YES, DELETE\n",
    "YES， DELETE",
  ])("rejects %j", (input) => {
    expect(isDeleteConfirmed(input)).toBe(false);
  });
});
