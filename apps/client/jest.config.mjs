/** @type {import('jest').Config} */
export default {
  // iOS resolution (`*.native.tsx`); web-only logic lives in pure functions tested here too.
  preset: "jest-expo",
  testMatch: ["<rootDir>/test/**/*.test.{ts,tsx}"],
  moduleNameMapper: { "^@/(.*)$": "<rootDir>/src/$1" },
};
