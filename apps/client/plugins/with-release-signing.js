// Expo config plugin (T4.2.5, ADR-0023): sign Android release builds with the persistent
// Routune key when ROUTUNE_KEYSTORE_FILE (and its passwords) are set, so a new APK installs
// over the previous one. Without them the template's debug signing stays (local builds).
const { withAppBuildGradle } = require("expo/config-plugins");

const MARKER = "// routune-release-signing";

const SIGNING_CONFIG = `        release { ${MARKER}
            if (System.getenv("ROUTUNE_KEYSTORE_FILE")) {
                storeFile file(System.getenv("ROUTUNE_KEYSTORE_FILE"))
                storePassword System.getenv("ROUTUNE_KEYSTORE_PASSWORD")
                keyAlias System.getenv("ROUTUNE_KEY_ALIAS")
                keyPassword System.getenv("ROUTUNE_KEY_PASSWORD")
            }
        }
`;

/** Adds the release signing config and uses it for release builds; idempotent. */
function applyReleaseSigning(gradle) {
  if (gradle.includes(MARKER)) return gradle;
  const withConfig = gradle.replace(/signingConfigs \{\n/u, (match) => match + SIGNING_CONFIG);
  const releaseType =
    /(buildTypes \{[\s\S]*?release \{[\s\S]*?)signingConfig signingConfigs\.debug/u;
  if (withConfig === gradle || !releaseType.test(withConfig)) {
    throw new Error("with-release-signing: unexpected android/app/build.gradle layout");
  }
  return withConfig.replace(
    releaseType,
    '$1signingConfig System.getenv("ROUTUNE_KEYSTORE_FILE") ? signingConfigs.release : signingConfigs.debug',
  );
}

module.exports = function withReleaseSigning(config) {
  return withAppBuildGradle(config, (mod) => {
    mod.modResults.contents = applyReleaseSigning(mod.modResults.contents);
    return mod;
  });
};
module.exports.applyReleaseSigning = applyReleaseSigning;
