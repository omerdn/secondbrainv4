import type {
  PomodoroConfig,
  SessionMode,
  SessionSegment,
  SessionTimer,
} from "@secondbrain/api-types";

/**
 * Timer arithmetic of the work session screen (PRD W2-W4, ARCHITECTURE §11). The server owns
 * the timer; the client only turns its state into what the clock shows, using its offset from
 * the server's clock, so a reload shows the same time.
 */

/** Server clock minus local clock, in ms, from a response's `server_time`. */
export function clockOffset(serverTime: string, localNow: number): number {
  const server = Date.parse(serverTime);
  return Number.isNaN(server) ? 0 : server - localNow;
}

/** Active time of the current segment at `serverNow`: accumulated plus the running stretch. */
export function elapsedMs(timer: SessionTimer, serverNow: number): number {
  if (timer.state !== "running" || timer.started_at === null) return timer.accumulated_ms;
  const started = Date.parse(timer.started_at);
  if (Number.isNaN(started)) return timer.accumulated_ms;
  return timer.accumulated_ms + Math.max(0, serverNow - started);
}

/** What the clock shows: time left for a countdown, time spent otherwise. */
export interface ClockFace {
  /** Milliseconds to show, never negative. */
  ms: number;
  countdown: boolean;
  /** A countdown that has run out; the segment goes on until it is stopped (W5). */
  overtime: boolean;
}

/** `planned - (accumulated + (now_server - started_at))` for countdowns (ARCHITECTURE §11). */
export function clockFace(timer: SessionTimer, serverNow: number): ClockFace {
  const elapsed = elapsedMs(timer, serverNow);
  if (timer.planned_ms === null) return { ms: elapsed, countdown: false, overtime: false };
  const left = timer.planned_ms - elapsed;
  return { ms: Math.max(0, left), countdown: true, overtime: left <= 0 };
}

/** `mm:ss`, or `h:mm:ss` from an hour; partial seconds round up on a countdown. */
export function formatClock(ms: number, countdown = false): string {
  const seconds = countdown ? Math.ceil(ms / 1000) : Math.floor(ms / 1000);
  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;
  const pad = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${pad(m)}:${pad(s)}`;
}

/** Whole minutes of a log entry, at least 1 once anything was counted. */
export function minutes(ms: number): number {
  if (ms <= 0) return 0;
  return Math.max(1, Math.round(ms / 60_000));
}

/** What a new segment can be (W2): the pomodoro phases, the stopwatch and a free break. */
export type Preset = "pomodoro" | "shortBreak" | "longBreak" | "stopwatch" | "break";

export const PRESETS: readonly Preset[] = [
  "pomodoro",
  "shortBreak",
  "longBreak",
  "stopwatch",
  "break",
];

/** The `start` command's mode and planned time for a preset. */
export function presetStart(
  preset: Preset,
  pomodoro: PomodoroConfig,
): { mode: SessionMode; planned_ms?: number } {
  switch (preset) {
    case "pomodoro":
      return { mode: "pomodoro", planned_ms: pomodoro.work_ms };
    case "shortBreak":
      return { mode: "break", planned_ms: pomodoro.short_break_ms };
    case "longBreak":
      return { mode: "break", planned_ms: pomodoro.long_break_ms };
    case "stopwatch":
      return { mode: "stopwatch" };
    case "break":
      return { mode: "break" };
  }
}

/** Pomodoro lengths are whole minutes between 1 and 240 (the server's limits). */
export const MIN_MINUTES = 1;
export const MAX_MINUTES = 240;

/** Minutes typed in a length field, or null when not a whole number in range. */
export function parseMinutes(text: string): number | null {
  const trimmed = text.trim();
  if (!/^\d{1,3}$/.test(trimmed)) return null;
  const value = Number(trimmed);
  return value >= MIN_MINUTES && value <= MAX_MINUTES ? value : null;
}

/** One entry of the segment log (W4): "45 dk · Gitar kursu", "5 dk mola". */
export interface LogEntry {
  id: string;
  mode: SessionMode;
  itemId: string | null;
  minutes: number;
  /** Still going: its time is as of now. */
  current: boolean;
}

/**
 * The session's segments, oldest first, with the open one's time as of `serverNow`. Segments
 * the server keeps for a stopped timer are all closed, so only a running or paused timer has a
 * current entry.
 */
export function segmentLog(
  segments: readonly SessionSegment[],
  timer: SessionTimer,
  serverNow: number,
): LogEntry[] {
  return segments.map((segment) => {
    const current = segment.ended_at === null && timer.state !== "stopped";
    const ms = current ? elapsedMs(timer, serverNow) : segment.active_ms;
    return {
      id: segment.id,
      mode: segment.mode,
      itemId: segment.item_id,
      minutes: minutes(ms),
      current,
    };
  });
}

/** Item ids the log names, each once, in order. */
export function loggedItems(segments: readonly SessionSegment[]): string[] {
  const ids: string[] = [];
  for (const segment of segments) {
    if (segment.item_id !== null && !ids.includes(segment.item_id)) ids.push(segment.item_id);
  }
  return ids;
}
