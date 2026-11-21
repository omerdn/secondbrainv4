// @ts-check
import path from "node:path";

/**
 * Client module boundaries (ARCHITECTURE §4.6, T0.4.8).
 *
 * - Code outside `features/x` may import only its public entry (`@/features/x`, i.e. its
 *   `index.ts`), never an internal file such as `@/features/x/session`.
 * - Shared code in `src/lib` must not import any feature (dependencies point from features to
 *   shared code, never back).
 *
 * `no-restricted-imports` cannot express "another feature than the one this file is in" or see
 * through relative paths (`../y/file`), so the rule resolves every import against the source
 * directory. Package imports are ignored.
 */

/** @typedef {{ srcDir: string, alias?: string }} Options */

/**
 * Feature of an absolute path: `features/<name>`, or `features/modules/<name>` for add-on
 * modules (ARCHITECTURE §13). `rest` is the path inside the feature.
 *
 * @param {string} featuresDir
 * @param {string} file
 * @returns {{ name: string, rest: string[] } | null}
 */
export function featureOf(featuresDir, file) {
  const rel = path.relative(featuresDir, file);
  if (rel === "" || rel.startsWith("..") || path.isAbsolute(rel)) return null;
  const parts = rel.split(path.sep);
  const size = parts[0] === "modules" && parts.length > 1 ? 2 : 1;
  return { name: parts.slice(0, size).join("/"), rest: parts.slice(size) };
}

/**
 * Whether `rest` (path inside a feature) is its public entry.
 *
 * @param {string[]} rest
 */
function isPublicEntry(rest) {
  return (
    rest.length === 0 || (rest.length === 1 && /^index(\.[cm]?[jt]sx?)?$/u.test(rest[0] ?? ""))
  );
}

/**
 * Absolute target of an import specifier, or `null` for packages.
 *
 * @param {string} specifier
 * @param {string} fromFile
 * @param {string} srcDir
 * @param {string} alias
 */
function resolveTarget(specifier, fromFile, srcDir, alias) {
  if (specifier.startsWith(alias)) return path.join(srcDir, specifier.slice(alias.length));
  if (specifier.startsWith("./") || specifier.startsWith("../")) {
    return path.resolve(path.dirname(fromFile), specifier);
  }
  return null;
}

/** @type {import("eslint").Rule.RuleModule} */
export const featureBoundariesRule = {
  meta: {
    type: "problem",
    docs: { description: "Enforce client feature boundaries (ARCHITECTURE §4.6)." },
    schema: [
      {
        type: "object",
        properties: { srcDir: { type: "string" }, alias: { type: "string" } },
        required: ["srcDir"],
        additionalProperties: false,
      },
    ],
    messages: {
      internal:
        "`features/{{target}}` iç dosyası import edilemez; public API'yi kullanın: `{{alias}}features/{{target}}`.",
      fromLib:
        "`src/lib` paylaşılan koddur ve feature import edemez (`features/{{target}}`); ortak kodu `src/lib` veya `packages/*` altına taşıyın.",
    },
  },
  create(context) {
    /** @type {unknown[]} */
    const raw = context.options;
    const options = /** @type {Options} */ (raw[0]);
    const srcDir = path.resolve(options.srcDir);
    const alias = options.alias ?? "@/";
    const featuresDir = path.join(srcDir, "features");
    const libDir = path.join(srcDir, "lib");
    const file = path.resolve(context.filename);
    const own = featureOf(featuresDir, file);
    const inLib = !path.relative(libDir, file).startsWith("..");

    /** @param {import("eslint").Rule.Node} node */
    function check(node) {
      const source =
        "source" in node ? /** @type {{ value?: unknown } | null} */ (node.source) : null;
      if (typeof source?.value !== "string") return;
      const target = resolveTarget(source.value, file, srcDir, alias);
      if (target === null) return;
      const feature = featureOf(featuresDir, target);
      if (feature === null || feature.name === own?.name) return;
      if (inLib) {
        context.report({ node, messageId: "fromLib", data: { target: feature.name } });
      } else if (!isPublicEntry(feature.rest)) {
        context.report({ node, messageId: "internal", data: { target: feature.name, alias } });
      }
    }

    return {
      ImportDeclaration: check,
      ExportNamedDeclaration: check,
      ExportAllDeclaration: check,
      ImportExpression: check,
    };
  },
};

/** ESLint plugin holding the rule. */
export const plugin = { rules: { "feature-boundaries": featureBoundariesRule } };

/**
 * Flat-config block enabling the rule for an app.
 *
 * @param {{ srcDir: string, files?: string[], alias?: string }} options
 *   `srcDir`: the app's `src` directory (where `@/` points). `files`: what to check; tests
 *   are usually left out because they exercise internals on purpose.
 * @returns {import("eslint").Linter.Config}
 */
export function featureBoundaries({ srcDir, files = ["src/**", "app/**"], alias = "@/" }) {
  return {
    files,
    plugins: { secondbrain: plugin },
    rules: { "secondbrain/feature-boundaries": ["error", { srcDir, alias }] },
  };
}
