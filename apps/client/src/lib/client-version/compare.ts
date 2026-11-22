/** `major.minor.patch` → numbers; anything else (pre-release tags, junk) → `null`. */
export function parseVersion(value: string): [number, number, number] | null {
  const match = /^(\d+)\.(\d+)\.(\d+)$/u.exec(value.trim());
  if (match === null) return null;
  return [Number(match[1]), Number(match[2]), Number(match[3])];
}

/**
 * True when `current` is older than `min` (T4.2.7). Unknown or unparsable versions are never
 * treated as outdated: blocking an app by mistake is worse than missing a check.
 */
export function isOutdated(current: string | null, min: string | null | undefined): boolean {
  if (current === null || min === null || min === undefined) return false;
  const a = parseVersion(current);
  const b = parseVersion(min);
  if (a === null || b === null) return false;
  for (let i = 0; i < 3; i += 1) {
    if (a[i] !== b[i]) return (a[i] ?? 0) < (b[i] ?? 0);
  }
  return false;
}
