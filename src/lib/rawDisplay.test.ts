import { describe, it, expect } from "vitest";
import { rawDisplay } from "./rawDisplay";

describe("rawDisplay", () => {
  it("converts signed -32768 to unsigned 0", () => {
    expect(rawDisplay(-32768)).toBe(0);
  });

  it("converts signed 0 to unsigned 32768", () => {
    expect(rawDisplay(0)).toBe(32768);
  });

  it("converts signed 32767 to unsigned 65535", () => {
    expect(rawDisplay(32767)).toBe(65535);
  });

  it("converts signed -1 to unsigned 32767", () => {
    expect(rawDisplay(-1)).toBe(32767);
  });

  it("converts signed 1 to unsigned 32769", () => {
    expect(rawDisplay(1)).toBe(32769);
  });
});
