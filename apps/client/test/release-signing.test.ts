// T4.2.5: release APKs are signed with the persistent key when ROUTUNE_KEYSTORE_* are set.
// eslint-disable-next-line @typescript-eslint/no-require-imports
const { applyReleaseSigning } = require("../plugins/with-release-signing") as {
  applyReleaseSigning: (gradle: string) => string;
};

const TEMPLATE = `android {
    signingConfigs {
        debug {
            storeFile file('debug.keystore')
        }
    }
    buildTypes {
        debug {
            signingConfig signingConfigs.debug
        }
        release {
            signingConfig signingConfigs.debug
            minifyEnabled false
        }
    }
}
`;

describe("release signing plugin", () => {
  it("adds an env-driven release config and uses it for release builds only", () => {
    const out = applyReleaseSigning(TEMPLATE);
    expect(out).toContain('storeFile file(System.getenv("ROUTUNE_KEYSTORE_FILE"))');
    expect(out).toContain(
      'signingConfig System.getenv("ROUTUNE_KEYSTORE_FILE") ? signingConfigs.release : signingConfigs.debug',
    );
    // The debug build type keeps the debug key.
    expect(out.match(/signingConfig signingConfigs\.debug\n/gu)).toHaveLength(1);
  });

  it("is idempotent and refuses an unknown layout", () => {
    const once = applyReleaseSigning(TEMPLATE);
    expect(applyReleaseSigning(once)).toBe(once);
    expect(() => applyReleaseSigning("android {}")).toThrow(/unexpected/u);
  });
});
