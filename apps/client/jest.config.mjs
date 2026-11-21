/** @type {import('jest').Config} */
export default {
  preset: "jest-expo",
  testMatch: ["<rootDir>/test/**/*.test.{ts,tsx}"],
  moduleNameMapper: { "^@/(.*)$": "<rootDir>/src/$1" },
};
