/**
 * Client-side idempotency keys (ARCHITECTURE §10.2, ADR-0003). Every mutation carries an
 * `Idempotency-Key`; the server stores the first response per key and replays it for repeats
 * (`app::idempotency`), so a retried submission is never applied twice.
 */

/** A random UUID v4 (the server accepts any visible ASCII up to 255 characters). */
export function newIdempotencyKey(): string {
  const cryptoApi = globalThis.crypto as Crypto | undefined;
  if (cryptoApi !== undefined && typeof cryptoApi.randomUUID === "function") {
    return cryptoApi.randomUUID();
  }
  if (cryptoApi !== undefined && typeof cryptoApi.getRandomValues === "function") {
    const bytes = cryptoApi.getRandomValues(new Uint8Array(16));
    // RFC 9562 §5.4: version 4, variant 10xx.
    bytes[6] = ((bytes[6] ?? 0) & 0x0f) | 0x40;
    bytes[8] = ((bytes[8] ?? 0) & 0x3f) | 0x80;
    const hex = Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("");
    return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
  }
  throw new Error("no secure random source for idempotency keys");
}

/** Key of the last unsuccessful submission and the input it was for. */
export interface PendingKey {
  input: string;
  key: string;
}

/**
 * Key for submitting `input` (a stable serialization of the mutation variables).
 *
 * Re-submitting the **same** input after a failure reuses the key: if the first attempt did
 * reach the server, the repeat gets the stored result instead of applying the change again.
 * Changed input gets a new key (the server rejects one key with a different body). After a
 * success the caller clears `pending`, so the next submission is a new operation.
 */
export function keyForSubmission(
  pending: PendingKey | null,
  input: string,
  generate: () => string = newIdempotencyKey,
): PendingKey {
  if (pending !== null && pending.input === input) {
    return pending;
  }
  return { input, key: generate() };
}
