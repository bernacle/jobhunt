import { describe, expect, it } from "vitest";

import { ago, compensationLine, eligibilityLine, inAbout, placeLine, verificationLine } from "./format";

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
});
