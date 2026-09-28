/*
 * What an opportunity's default view shows of its explanation. The API
 * returns every list strongest first: reasons personal-first by weight,
 * caveats blockers-first then most negative, unknowns the person's stated
 * requirements first. Surfaces show a selection from the top of those
 * lists; the rest stays in the evidence. Nothing here ranks, classifies
 * materiality beyond the API's own order, or counts what isn't shown.
 */

export type ConcernKind = "caution" | "unresolved" | "missing";

export interface Concern {
  kind: ConcernKind;
  text: string;
  /** The ranking's "check this first" note. */
  checkFirst?: boolean;
}

export interface DecisionInput {
  /** Why it may be worth the person's time, strongest first. */
  why: string[];
  /** What counts against it, and conditions, strongest first. */
  caveats: string[];
  /** What the posting doesn't say that matters, stated requirements first. */
  unknowns?: string[];
  /** The ranking's "check this first" note, when it applies. */
  checkFirst?: string | null;
  /**
   * The eligibility headline the facts line already shows: a check-first
   * note that only repeats it isn't said twice.
   */
  eligibilityHeadline?: string | null;
}

export type DecisionVariant = "lead" | "peer";

/** Two reasons and one usual concern on the lead; one of each on a peer. */
export const DECISION_LIMITS: Record<DecisionVariant, { reasons: number; concerns: number }> = {
  lead: { reasons: 2, concerns: 1 },
  peer: { reasons: 1, concerns: 1 },
};

function same(a: string, b: string): boolean {
  const norm = (s: string) => s.trim().toLowerCase().replace(/[.\s]+$/, "");
  return norm(a) === norm(b);
}

function distinct(lines: string[]): string[] {
  return lines.filter((line, i) => line.trim() !== "" && lines.findIndex((other) => same(other, line)) === i);
}

/**
 * A requirement the person stated that the posting leaves open. The API
 * words these "Unresolved: you require …" and lists them before what a
 * posting merely omits; they can decide whether the job is possible at
 * all, so they come before a soft caveat.
 */
export function isStatedUnresolved(line: string): boolean {
  return /^unresolved\b/i.test(line.trim());
}

/**
 * Every concern, most material first: what to check first, the person's
 * unresolved requirements, cautions, then what the posting doesn't say.
 * Nothing is dropped for lack of a flag; duplicates are said once.
 */
export function concernsOf(input: DecisionInput): Concern[] {
  const unknowns = distinct(input.unknowns ?? []);
  const cautions = distinct(input.caveats).filter((c) => !unknowns.some((u) => same(u, c)));
  const out: Concern[] = [];
  const note = input.checkFirst?.trim();
  if (note && !(input.eligibilityHeadline && same(note, input.eligibilityHeadline))) {
    out.push({ kind: "caution", text: note, checkFirst: true });
  }
  for (const u of unknowns) if (isStatedUnresolved(u)) out.push({ kind: "unresolved", text: u });
  for (const c of cautions) out.push({ kind: "caution", text: c });
  for (const u of unknowns) if (!isStatedUnresolved(u)) out.push({ kind: "missing", text: u });
  return out.filter((c, i) => out.findIndex((other) => same(other.text, c.text)) === i);
}

/**
 * The strongest distinct reasons and the most material concerns. A separate
 * unresolved stated requirement still appears when a check-first note took
 * the usual single concern slot; otherwise that requirement would disappear
 * from the default view solely because the gate note came first.
 */
export function selectDecision(input: DecisionInput, variant: DecisionVariant): { reasons: string[]; concerns: Concern[] } {
  const limit = DECISION_LIMITS[variant];
  const concerns = concernsOf(input);
  const selected = concerns.slice(0, limit.concerns);
  if (selected.some((c) => c.checkFirst) && !selected.some((c) => c.kind === "unresolved")) {
    const unresolved = concerns.find((c) => c.kind === "unresolved");
    if (unresolved) selected.push(unresolved);
  }
  return {
    reasons: distinct(input.why).slice(0, limit.reasons),
    concerns: selected,
  };
}
