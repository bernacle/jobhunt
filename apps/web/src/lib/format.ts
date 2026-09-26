import type {
  CompensationView,
  EligibilityBrief,
  FitTier,
  PipelineStage,
  VerificationBrief,
} from "./api-types";

/** "just now", "18 min ago", "3 h ago", "2 days ago", "on 12 Sep". */
export function ago(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return "";
  const then = new Date(iso);
  const seconds = Math.round((now.getTime() - then.getTime()) / 1000);
  if (Number.isNaN(seconds)) return "";
  if (seconds < 0) return `in ${until(then, now)}`;
  if (seconds < 90) return "just now";
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes} min ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 24) return `${hours} h ago`;
  const days = Math.round(hours / 24);
  if (days < 14) return `${days} ${days === 1 ? "day" : "days"} ago`;
  return `on ${then.toLocaleDateString("en", { day: "numeric", month: "short", timeZone: "UTC" })}`;
}

function until(then: Date, now: Date): string {
  const minutes = Math.max(1, Math.round((then.getTime() - now.getTime()) / 60_000));
  if (minutes < 60) return `${minutes} min`;
  const hours = Math.round(minutes / 60);
  return `${hours} h`;
}

/** When something is due: "in 25 min", "in 3 h". */
export function inAbout(iso: string | null | undefined, now: Date = new Date()): string {
  if (!iso) return "";
  const then = new Date(iso);
  if (then.getTime() <= now.getTime()) return "shortly";
  return `in about ${until(then, now)}`;
}

export const TIER_LABEL: Record<FitTier, string> = {
  strong_fit: "Strong fit",
  worth_reviewing: "Worth reviewing",
  maybe: "Maybe",
  low_priority: "Low priority",
};

const AUTHORITY: Record<string, string> = {
  employer_first_party: "the employer's own site",
  employer_configured_ats: "the employer's job board",
  trusted_source: "a trusted source",
  secondary_source: "a secondary source",
  unknown: "a source Narrow couldn't trace to the employer",
};

export function authorityLabel(authority: string | null | undefined): string {
  return AUTHORITY[authority ?? "unknown"] ?? AUTHORITY.unknown!;
}

/** One sentence about how current the listing is. */
export function verificationLine(v: VerificationBrief, now: Date = new Date()): string {
  switch (v.state) {
    case "verified_active":
      return v.trusted
        ? `Verified ${ago(v.verified_at, now)} on ${authorityLabel(v.authority)}`
        : `Last verified ${ago(v.verified_at, now)}; ${lower(v.not_trusted_because) || "not recently enough"}`;
    case "verified_closed":
    case "closed_by_discovery":
      return "The listing appears closed";
    case "could_not_verify":
      return `Couldn't be verified recently${v.not_trusted_because ? `: ${lower(v.not_trusted_because)}` : ""}`;
    default:
      return "Not verified at the employer yet";
  }
}

/**
 * The check a verification earns, from the API's own trust and freshness:
 * the mint check only for a current, trusted verification; the grey check
 * when it is aging; none when it is stale, untrusted, closed or missing.
 */
export type VerificationMark = "fresh" | "aging" | null;

export function verificationMark(v: VerificationBrief): VerificationMark {
  if (v.state !== "verified_active" || !v.trusted) return null;
  const freshness = v.freshness ?? "fresh";
  return freshness === "stale" ? null : freshness;
}

/**
 * The check one source record earns: the listing's own mark, and only for
 * the open record the listing's verification rests on. Other records never
 * borrow it, however "verified" their own last state reads.
 */
export function sourceMark(record: { source: string; status: string }, v: VerificationBrief): VerificationMark {
  return record.status === "open" && v.source === record.source ? verificationMark(v) : null;
}

function lower(text: string | null | undefined): string {
  if (!text) return "";
  return text.charAt(0).toLowerCase() + text.slice(1);
}

/** Eligibility, in normal language, and whether it needs attention. */
export function eligibilityLine(e: EligibilityBrief): { text: string; attention: boolean } {
  switch (e.status) {
    case "eligible":
      return { text: `You appear eligible — ${lower(e.headline)}`, attention: false };
    case "conditional":
      return { text: `Eligible on a condition — ${lower(e.headline)}`, attention: true };
    case "uncertain":
      return { text: `Eligibility unclear — ${lower(e.headline)}`, attention: true };
    case "ineligible":
      return { text: `Probably not eligible — ${lower(e.headline)}`, attention: true };
    default:
      return { text: "Eligibility not checked yet", attention: true };
  }
}

/** Pay, with how much to trust it. */
export function compensationLine(c: CompensationView): { text: string; known: boolean } {
  if (c.status === "published" && c.ranges.length > 0) {
    return { text: `${c.ranges[0]}${c.verified ? "" : " (as listed)"}`, known: true };
  }
  if (c.status === "published" && c.summary) return { text: c.summary, known: true };
  return { text: c.status === "not_published" ? "Pay not published" : "Pay unknown", known: false };
}

/**
 * Eligibility as a short fact, with how settled it is. Only `eligible` is
 * resolved; a condition or an unclear reading must never look confirmed.
 */
export type EligibilityKind = "resolved" | "conditional" | "unclear" | "ineligible" | "unchecked";

export function eligibilityFact(e: EligibilityBrief): { label: string; detail: string; kind: EligibilityKind } {
  const detail = lower(e.headline);
  switch (e.status) {
    case "eligible":
      return { label: "Eligible", detail, kind: "resolved" };
    case "conditional":
      return { label: "Eligible on a condition", detail, kind: "conditional" };
    case "uncertain":
      return { label: "Eligibility unclear", detail, kind: "unclear" };
    case "ineligible":
      return { label: "Probably not eligible", detail, kind: "ineligible" };
    default:
      return { label: "Eligibility not checked yet", detail: "", kind: "unchecked" };
  }
}

/**
 * "Remote" with nothing saying where from: the listing doesn't settle
 * where you can work from, so it must not read as resolved.
 */
export function unscopedRemote(locations: string[] | undefined, workplace?: string | null): boolean {
  const places = (locations ?? []).map((l) => l.trim().toLowerCase()).filter(Boolean);
  if (places.length === 0) return workplace === "remote";
  return places.every((p) => p === "remote");
}

/** `greenhouse:stripe` → "Greenhouse · stripe". */
export function sourceLabel(source: string | null | undefined): string {
  if (!source) return "";
  const [kind, instance] = source.split(":", 2);
  const name = kind ? kind.charAt(0).toUpperCase() + kind.slice(1) : source;
  return instance ? `${name} · ${instance}` : name;
}

export function placeLine(locations: string[] | undefined, workplace?: string | null): string {
  const parts = (locations ?? []).slice(0, 2);
  const joined = parts.join(" · ");
  if (workplace && !joined.toLowerCase().includes(workplace)) {
    return joined ? `${joined} · ${workplace}` : workplace;
  }
  return joined;
}

export const STAGE_LABEL: Record<PipelineStage, string> = {
  unseen: "New",
  seen: "Looked at",
  saved: "Saved",
  rejected: "Not for me",
  applied: "Applied",
  interviewing: "Interviewing",
  offer: "Offer",
};

/** How strongly a stated preference holds. */
export const STANCE_LABEL: Record<string, string> = {
  required: "Must",
  wanted: "Want",
  acceptable: "Fine with",
  unwanted: "Avoid",
};

/** Backend lines are sentences; some start lowercase ("senior level, …"). */
export function sentence(text: string): string {
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : text;
}
