import { describe, expect, it } from "vitest";

import {
  ago,
  compensationLine,
  eligibilityFact,
  eligibilityLine,
  inAbout,
  placeLine,
  sourceLabel,
  sourceMark,
  unscopedRemote,
  verificationLine,
  verificationMark,
} from "./format";

const now = new Date("2026-09-25T12:00:00Z");

describe("format", () => {
  it("says how long ago, in words", () => {
    expect(ago("2026-09-25T11:59:30Z", now)).toBe("just now");
    expect(ago("2026-09-25T11:42:00Z", now)).toBe("18 min ago");
    expect(ago("2026-09-25T09:00:00Z", now)).toBe("3 h ago");
    expect(ago("2026-09-23T12:00:00Z", now)).toBe("2 days ago");
    expect(ago("2026-08-01T12:00:00Z", now)).toBe("on Aug 1");
    expect(inAbout("2026-09-25T12:25:00Z", now)).toBe("in about 25 min");
    expect(inAbout("2026-09-25T11:00:00Z", now)).toBe("shortly");
  });

  it("describes verification without jargon", () => {
    expect(
      verificationLine({ state: "verified_active", trusted: true, verified_at: "2026-09-25T11:42:00Z", authority: "employer_first_party" }, now),
    ).toBe("Verified 18 min ago on the employer's own site");
    expect(verificationLine({ state: "not_verified", trusted: false }, now)).toBe("Not verified at the employer yet");
    expect(verificationLine({ state: "verified_closed", trusted: true }, now)).toBe("The listing appears closed");
  });

  it("flags eligibility that needs attention", () => {
    expect(eligibilityLine({ status: "eligible", headline: "Remote from anywhere" })).toEqual({
      text: "You appear eligible — remote from anywhere",
      attention: false,
    });
    expect(eligibilityLine({ status: "uncertain", headline: "The posting doesn't say" }).attention).toBe(true);
    expect(eligibilityLine({ status: "not_checked", headline: "" }).text).toBe("Eligibility not checked yet");
  });

  it("keeps unknown pay unknown", () => {
    expect(compensationLine({ status: "not_observed", ranges: [], verified: false })).toEqual({ text: "Pay unknown", known: false });
    expect(compensationLine({ status: "published", ranges: [], summary: "Competitive", verified: false }).text).toBe("Competitive");
  });

  it("joins places with the workplace when it adds something", () => {
    expect(placeLine(["Lisbon, Portugal"], "hybrid")).toBe("Lisbon, Portugal · hybrid");
    expect(placeLine(["Remote - Worldwide"], "remote")).toBe("Remote - Worldwide");
    expect(placeLine([], "remote")).toBe("remote");
  });

  it("never reads a condition or an unclear eligibility as settled", () => {
    expect(eligibilityFact({ status: "eligible", headline: "Remote from anywhere" })).toEqual({ label: "Eligible", detail: "remote from anywhere", kind: "resolved" });
    expect(eligibilityFact({ status: "conditional", headline: "If you relocate" }).kind).toBe("conditional");
    expect(eligibilityFact({ status: "uncertain", headline: "The posting doesn't say" }).label).toBe("Eligibility unclear");
    expect(eligibilityFact({ status: "not_checked", headline: "" }).kind).toBe("unchecked");
  });

  it("flags remote without a scope", () => {
    expect(unscopedRemote(["Remote"], "remote")).toBe(true);
    expect(unscopedRemote([], "remote")).toBe(true);
    expect(unscopedRemote(["Remote - Worldwide"], "remote")).toBe(false);
    expect(unscopedRemote(["Remote", "Lisbon, Portugal"], "remote")).toBe(false);
    expect(unscopedRemote([], "onsite")).toBe(false);
  });

  it("names a source readably", () => {
    expect(sourceLabel("greenhouse:stripe")).toBe("Greenhouse · stripe");
    expect(sourceLabel("lever")).toBe("Lever");
  });

  it("earns a verification check only when current and trusted", () => {
    const v = { state: "verified_active" as const, trusted: true, source: "greenhouse:acme" };
    expect(verificationMark({ ...v, freshness: "fresh" })).toBe("fresh");
    expect(verificationMark({ ...v, freshness: "aging" })).toBe("aging");
    expect(verificationMark({ ...v, freshness: "stale" })).toBeNull();
    expect(verificationMark({ ...v, trusted: false })).toBeNull();
    expect(verificationMark({ ...v, state: "could_not_verify" })).toBeNull();
    // A source record borrows the mark only if the verification rests on it.
    expect(sourceMark({ source: "greenhouse:acme", status: "open" }, { ...v, freshness: "fresh" })).toBe("fresh");
    expect(sourceMark({ source: "greenhouse:acme", status: "open" }, { ...v, freshness: "stale" })).toBeNull();
    expect(sourceMark({ source: "otherboard:all", status: "open" }, { ...v, freshness: "fresh" })).toBeNull();
    expect(sourceMark({ source: "greenhouse:acme", status: "closed" }, { ...v, freshness: "fresh" })).toBeNull();
  });
});
