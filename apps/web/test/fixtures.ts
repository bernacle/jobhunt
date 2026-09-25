import type { FeedItem, FeedView, FeedbackResult, LearnedView, TasteView } from "@/lib/api-types";

/** A recommendation as the API returns it (values from a real response). */
export function feedItem(overrides: Partial<FeedItem> = {}): FeedItem {
  return {
    id: "opp_0123456789abcdef0123456789abcdef",
    short_id: "opp_01234567",
    title: "Senior Backend Engineer (Go)",
    company: "Ledgerly",
    tier: "strong_fit",
    recommendation: "recommended",
    summary: "backend · senior · Go · USD 150,000 – 190,000 per year · remote",
    verification: {
      state: "verified_active",
      trusted: true,
      verified_at: "2026-09-25T11:42:00Z",
      freshness: "fresh",
      authority: "employer_configured_ats",
      source: "greenhouse:ledgerly",
    },
    eligibility: { status: "eligible", headline: "The listing is remote from anywhere" },
    compensation: {
      status: "published",
      ranges: ["USD 150,000 – 190,000 per year"],
      verified: true,
      verified_at: "2026-09-25T11:42:00Z",
    },
    locations: ["Remote - Worldwide"],
    workplace: "remote",
    department: "Engineering",
    why: ["Backend roles: a role you want", "Small teams: a kind of company or team you want"],
    consider: ["senior level, a step below your latest title", "The posting doesn't say whether it's product companies"],
    sources: 1,
    next_step: "jobhunt why opp_01234567",
    reason: "new",
    stage: "unseen",
    ...overrides,
  };
}

export function feedView(overrides: Partial<FeedView> = {}): FeedView {
  return {
    generated_at: "2026-09-25T12:00:00Z",
    items: [feedItem()],
    caught_up: false,
    summary: { checked: 7500, passed_eligibility: 312, worth_reviewing: 18, new: 3, changed: 0, shown: 3 },
    passed_over: 0,
    pipeline: { saved: 2, applied: 1, interviewing: 0, offer: 0 },
    discovery: { mode: "background", last_read_at: "2026-09-25T11:40:00Z", next_read_at: "2026-09-25T12:30:00Z" },
    refresh: { performed: false, reason: "job boards are read in the background" },
    learning: { feedback: 4, active_patterns: 1, has_preferences: true },
    verified_now: 0,
    ...overrides,
  };
}

export function feedbackResult(overrides: Partial<FeedbackResult> = {}): FeedbackResult {
  return {
    id: "opp_0123456789abcdef0123456789abcdef",
    title: "Senior Backend Engineer (Go)",
    company: "Ledgerly",
    action: "save",
    recorded: true,
    previous: { stage: "unseen", furthest: "unseen", feedback: [] },
    state: { stage: "saved", furthest: "unseen", feedback: [] },
    taste_changed: false,
    ...overrides,
  };
}

export function learned(overrides: Partial<LearnedView> = {}): LearnedView {
  return {
    key: "role:sre",
    dimension: "role",
    value: "SRE / DevOps",
    direction: "avoid",
    status: "active",
    confidence: "established",
    basis: "2 reasons in your words",
    reasons: 2,
    opportunities: 2,
    last_reinforced: "2026-09-24T10:00:00Z",
    support: [
      {
        opportunity: "opp_1",
        title: "Site Reliability Engineer",
        company: "OpsCo",
        action: "reject",
        at: "2026-09-24T10:00:00Z",
        kind: "reason",
        reason: "too much SRE",
        read_as: "SRE",
        summary: "rejected Site Reliability Engineer at OpsCo: “too much SRE”",
      },
    ],
    against: [],
    ...overrides,
  };
}

export function tasteView(overrides: Partial<TasteView> = {}): TasteView {
  return {
    stated: [
      {
        id: "pref_1",
        category: "role",
        stance: "wanted",
        value: "backend roles",
        certainty: "certain",
        origin: "statement",
        snippet: "backend roles",
        active: true,
      },
    ],
    statements: [],
    learned: [learned()],
    contradictory: [],
    covered_by_stated: [],
    emerging: [],
    job_notes: [],
    unread_reasons: [],
    feedback_events: 4,
    opportunities: 3,
    ...overrides,
  };
}
