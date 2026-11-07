// A thread's one provider session (docs/chat.md 4.5.6): opened for a turn, kept between turns,
// closed after a while idle or when another thread needs the room. What a session reports is
// handed on as a step of its own, after the one running; what a dropped session reports
// reaches nothing, so a session killed mid-turn can't change the turn after it.

import { errorMessage, log } from "../log";
import type { RequestPayload, Thread } from "../protocol/model";
import type {
  Config,
  DriverEvent,
  Provider,
  ProviderSession,
  SessionChange,
  SessionSink,
  SessionSpec,
} from "../providers/provider";
import type { Waiter } from "./requests";

/** How long a session waits for its next turn before it's closed (4.1). */
const IDLE_MS = 5 * 60_000;

/**
 * The session `thread` runs its turns in, as conversation `sessionId`: in the chat's mode, or
 * Koyo's `defaultMode` when it names none (C12), with Koyo's tools while Koyo's settings say.
 */
export function specFor(thread: Thread, sessionId: string, config: Config): SessionSpec {
  return {
    threadId: thread.id,
    cwd: thread.cwd,
    sessionId,
    // The thread had a conversation before, so there's one to resume.
    resume: thread.sessionId !== null,
    model: thread.model,
    effort: thread.effort,
    fast: thread.fast,
    permission: thread.permission ?? config.defaultMode,
    plan: thread.plan,
    resumeAt: thread.resumeAt,
    koyoTools: config.koyoTools,
  };
}

/** Where what the session reports goes, each call in a step of its own. */
export interface Reports {
  readonly event: (event: DriverEvent) => void;
  readonly ask: (payload: RequestPayload, signal: AbortSignal, waiter: Waiter) => void;
  /** The session went, by whichever road: what lived in it beside its turn went with it. */
  readonly gone: () => void;
}

interface Session {
  readonly live: ProviderSession;
  /** Whether a settings change it couldn't take wants it opened again. */
  reopen: boolean;
}

export class SessionSlot {
  readonly #provider: Provider;
  /** Runs work as a step of its own, after the one running. */
  readonly #later: (work: () => void) => void;
  readonly #reports: Reports;
  #session: Session | undefined;
  /** Which session reports are from: a dropped one's reach nothing. */
  #epoch = 0;
  #idle: Timer | undefined;

  constructor(provider: Provider, later: (work: () => void) => void, reports: Reports) {
    this.#provider = provider;
    this.#later = later;
    this.#reports = reports;
  }

  /** The session, when one is open. */
  get live(): ProviderSession | undefined {
    return this.#session?.live;
  }

  /** The session open now, or a new one opened as `spec` says. */
  ensure(spec: SessionSpec): ProviderSession {
    clearTimeout(this.#idle);
    if (this.#session !== undefined && !this.#session.reopen) {
      return this.#session.live;
    }
    this.close();
    const current = this.#current();
    const sink: SessionSink = {
      event: (event) => {
        this.#later(() => {
          if (current()) {
            this.#reports.event(event);
          }
        });
      },
      ask: async (payload, signal, bySubagent) => await new Promise((resolve, reject) => {
        this.#later(() => {
          if (current()) {
            this.#reports.ask(payload, signal, { resolve, reject, bySubagent });
          } else {
            reject(new Error("The session was closed."));
          }
        });
      }),
    };
    const live = this.#provider.open(spec, sink);
    this.#session = { live, reopen: false };
    return live;
  }

  /** Forgets the session, whose reports reach nothing from now on, and hands it back. */
  drop(): ProviderSession | undefined {
    const session = this.#session;
    clearTimeout(this.#idle);
    this.#session = undefined;
    this.#epoch += 1;
    if (session !== undefined) {
      this.#reports.gone();
    }
    return session?.live;
  }

  close(): void {
    this.drop()?.close().catch((error: unknown) => {
      log.warn("Closing a session failed", error);
    });
  }

  /**
   * Asks the turn to stop; false when there's no session to ask. A session that can't be
   * asked is ended, and its turn with it.
   */
  interrupt(): boolean {
    const live = this.#session?.live;
    if (live === undefined) {
      return false;
    }
    const current = this.#current();
    live.interrupt().catch((error: unknown) => {
      log.warn("Interrupting a turn failed; its process is ended instead", error);
      this.#later(() => {
        if (current()) {
          this.drop()?.kill();
          this.#reports.event({ type: "session.ended", error: errorMessage(error) });
        }
      });
    });
    return true;
  }

  /** Gives the session a settings change, or has it opened again when it can't take it. */
  configure(change: SessionChange): void {
    const session = this.#session;
    if (session === undefined || Object.keys(change).length === 0) {
      return;
    }
    try {
      session.reopen ||= session.live.configure(change) === "reopen";
    } catch (error) {
      log.warn("A session didn't take a settings change; it's opened again", error);
      session.reopen = true;
    }
  }

  /** Asks subagent `key` to stop; one lives as long as the session, so there is one to ask. */
  stopSubagent(key: string): void {
    this.#session?.live.stopSubagent(key);
  }

  /** Whether reports come from the session open now, as they come. */
  #current(): () => boolean {
    const epoch = this.#epoch;
    return () => epoch === this.#epoch;
  }

  /** Closes the session a while from now, unless a turn wants it first or `busy` says so. */
  idleSoon(busy: () => boolean): void {
    clearTimeout(this.#idle);
    if (this.#session === undefined) {
      return;
    }
    this.#idle = setTimeout(() => {
      if (!busy()) {
        this.close();
      }
    }, IDLE_MS);
  }
}
