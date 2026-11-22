import { apiUrlFrom, DEFAULT_API_URL } from "@/lib/api";

describe("API URL (T0.5.8)", () => {
  it("falls back to the local API when unset, empty or blank", () => {
    for (const value of [undefined, "", "   "]) {
      expect(apiUrlFrom(value)).toBe(DEFAULT_API_URL);
    }
  });

  it("uses a configured value, trimmed", () => {
    expect(apiUrlFrom(" https://api.staging.example.com ")).toBe("https://api.staging.example.com");
  });
});
