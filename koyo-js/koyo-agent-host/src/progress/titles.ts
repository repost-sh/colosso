/** Words that name a kind of work, never the work itself: a title of these alone says nothing. */
const GENERIC = new Set(
  [
    "a an and the of for to on in with step steps phase phases slice slices task tasks part stage",
    "milestone item items todo subtask work misc stuff things changes change update updates wip",
    "main more other next first second third final last again new follow up followup qa cycle",
    "cycles round rounds pass passes iteration fix fixes fixups review reviews rereview test tests",
    "testing check checks verify verification gate gates run rerun implement implementation build",
    "plan planning design docs polish cleanup finalize wrap",
  ].join(" ").split(" "),
);

const PHASE_WORDS = 3;
const SUBSTEP_WORDS = 2;
/** The least share of an old title's significant words a new one keeps to be the same work. */
const SAME_WORK = 0.2;

function wordsOf(title: string): string[] {
  return title.toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((word) => word !== "");
}

/** Why a title is too vague to show, or null: too few words for its level, or generic ones. */
function vagueness(id: string, title: string, isPhase: boolean): string | null {
  const words = wordsOf(title);
  const least = isPhase ? PHASE_WORDS : SUBSTEP_WORDS;
  const example = isPhase
    ? "\"Histogram endpoint in the explorer API\""
    : "\"QA the empty-state histogram on /explorer\"";
  if (words.every((word) => GENERIC.has(word) || /^\d+$/u.test(word))) {
    return `Step ${id}'s title "${title}" names a kind of work, not the work: say what it builds, `
      + `fixes or checks, as ${example}.`;
  }
  if (words.length < least) {
    const level = isPhase ? "phase" : "sub-step";
    return `Step ${id}'s title "${title}" is too short for a ${level}: use at least `
      + `${String(least)} words that say what it covers, as ${example}.`;
  }
  return null;
}

/** The words of a title that name its work: longer than two letters, and none of GENERIC's. */
function significant(title: string): Set<string> {
  return new Set(wordsOf(title).filter((word) => word.length > 2 && !GENERIC.has(word)));
}

/**
 * Why a step's new title reads as other work than `held`, its old one, or null: it keeps under a
 * fifth of the old one's significant words, so the id was reused for something else.
 */
function drift(id: string, held: string, title: string): string | null {
  const before = significant(held);
  const after = significant(title);
  const kept = [...before].filter((word) => after.has(word)).length;
  if (before.size === 0 || kept / before.size >= SAME_WORK) {
    return null;
  }
  return `Step ${id} is "${held}", and "${title}" reads as different work: give that work a new `
    + `id, and set step ${id}'s status as it stands.`;
}

/**
 * Why `title`, given for step `id`, which had `held` until now, is refused, or null: it says too
 * little, or it names other work than the step's.
 */
export function titleRefusal(
  id: string,
  held: string | undefined,
  title: string,
  isPhase: boolean,
): string | null {
  const vague = vagueness(id, title, isPhase);
  const kept = vague !== null || held === undefined || held === title;
  return kept ? vague : drift(id, held, title);
}
