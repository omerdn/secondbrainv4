import type { ServerMessage, WsEvent } from "@secondbrain/api-types";
import type { QueryKey } from "@tanstack/react-query";

/**
 * Query invalidation from `/ws` events (ARCHITECTURE §2, §10.1). Events only say *what
 * changed* (ids, never content); the affected queries are invalidated and TanStack Query
 * refetches the active ones over HTTP.
 *
 * **Query key convention:** every query key starts with the resource kind, the same string
 * the server sends as `kind` (`["task", id]`, `["task", "list", filters]`, `["notifications"]`).
 * Invalidating `[kind]` then covers that kind's lists and details (prefix match). The shared
 * item screens do not know the kind before loading, so they use `["item", id]`.
 */

/** Query keys to invalidate for one event's `data`; empty when the data is not understood. */
export type InvalidationRule = (data: unknown) => QueryKey[];

export type InvalidationRules = Readonly<Record<string, InvalidationRule>>;

const isRecord = (value: unknown): value is Record<string, unknown> =>
  typeof value === "object" && value !== null && !Array.isArray(value);

const nonEmpty = (value: unknown): value is string => typeof value === "string" && value !== "";

/**
 * Built-in rules. Features add theirs as they arrive (`session.timer` carries timer state and
 * is handled by the sessions feature, not here).
 */
export const defaultRules: InvalidationRules = {
  /** `{ id, kind }`: an object of `kind` was created, changed or deleted. */
  "item.changed": (data) =>
    isRecord(data) && nonEmpty(data.kind) && nonEmpty(data.id)
      ? [[data.kind], ["item", data.id]]
      : [],
  "notification.new": () => [["notifications"]],
};

/** The query keys to invalidate for `event` under `rules`; unknown events change nothing. */
export function invalidationsFor(event: WsEvent, rules: InvalidationRules): QueryKey[] {
  // Own properties only: an event named "toString" must not hit the prototype.
  const rule = Object.prototype.hasOwnProperty.call(rules, event.name)
    ? rules[event.name]
    : undefined;
  return rule === undefined ? [] : rule(event.data);
}

/** The event carried by a server message, if it is one. */
export function eventOf(message: ServerMessage): WsEvent | null {
  return message.type === "event" ? message.event : null;
}
