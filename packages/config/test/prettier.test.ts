import { format } from "prettier";
import { describe, expect, it } from "vitest";

import config from "../src/prettier.js";

describe("@secondbrain/config/prettier", () => {
  it("formats with double quotes, semicolons and trailing commas", async () => {
    const out = await format("const a = {x: 'y', z: 1}", { ...config, parser: "typescript" });
    expect(out).toBe('const a = { x: "y", z: 1 };\n');
  });

  it("wraps at 100 columns", async () => {
    const long = `call(${Array.from({ length: 12 }, (_, i) => `argument${i}`).join(", ")});`;
    const out = await format(long, { ...config, parser: "typescript" });
    expect(out.split("\n").every((line) => line.length <= 100)).toBe(true);
    expect(out).toContain("argument11,\n");
  });
});
