import type { Endpoints as Api } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import type { StatusLine } from "@secondbrain/api-types";
import { useQuery } from "@tanstack/react-query";

/**
 * The latest process page line of items (PRD S3, T2.5.10). Cards ask one by one; the loader
 * gathers the ids asked in the same tick into one request of at most [`BATCH`] items. Items the
 * viewer cannot see, or without a page, simply have no line (the server leaves them out).
 */
const BATCH = 100;

type Waiting = { id: string; resolve: (line: StatusLine | null) => void };

class LineLoader {
  private queue: Waiting[] = [];
  private scheduled = false;

  constructor(private readonly api: Api) {}

  load(id: string): Promise<StatusLine | null> {
    return new Promise((resolve) => {
      this.queue.push({ id, resolve });
      if (!this.scheduled) {
        this.scheduled = true;
        setTimeout(() => {
          void this.flush();
        }, 0);
      }
    });
  }

  private async flush() {
    const waiting = this.queue;
    this.queue = [];
    this.scheduled = false;
    for (let start = 0; start < waiting.length; start += BATCH) {
      const chunk = waiting.slice(start, start + BATCH);
      const ids = [...new Set(chunk.map((w) => w.id))];
      let found = new Map<string, StatusLine>();
      try {
        const lines = await this.api.notebooks.processLines(ids);
        found = new Map(lines.map((l) => [l.item_id, l.line]));
      } catch {
        // No line is shown when it cannot be loaded; the card itself still works.
      }
      for (const w of chunk) w.resolve(found.get(w.id) ?? null);
    }
  }
}

const loaders = new WeakMap<Api, LineLoader>();

function loaderFor(api: Api): LineLoader {
  let loader = loaders.get(api);
  if (loader === undefined) {
    loader = new LineLoader(api);
    loaders.set(api, loader);
  }
  return loader;
}

/** The latest line of `itemId`'s process page, or null. */
export function useStatusLine(itemId: string): StatusLine | null {
  const api = useApi();
  const line = useQuery({
    queryKey: ["status-line", itemId],
    queryFn: () => loaderFor(api).load(itemId),
    staleTime: 30_000,
    retry: false,
  });
  return line.data ?? null;
}

/** Whole days from `date` to `today` (both `YYYY-MM-DD`). */
export function daysBetween(date: string, today: string): number {
  const ms = Date.parse(`${today}T00:00:00Z`) - Date.parse(`${date}T00:00:00Z`);
  return Math.round(ms / 86_400_000);
}

/** Today in the device's time zone as `YYYY-MM-DD`. */
export function localToday(now: Date = new Date()): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}
