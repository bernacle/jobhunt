import { describe, expect, it } from "vitest";

import { concernsOf, isStatedUnresolved, selectDecision } from "./decision";

const why = ["Backend roles: a role you want", "Small teams: a kind of company or team you want", "Payments: a domain you want"];

describe("selectDecision", () => {
  it("takes the strongest distinct reasons, in the API's order", () => {
    expect(selectDecision({ why: [...why, "backend roles: a role you want."], caveats: [] }, "lead").reasons).toEqual(why.slice(0, 2));
    expect(selectDecision({ why, caveats: [] }, "peer").reasons).toEqual(why.slice(0, 1));
  });

  it("shows one concern by default and keeps the rest for the evidence", () => {
    const input = { why, caveats: ["On-call one week in six", "Four-hour take-home"], unknowns: ["Equity not stated"] };
    expect(selectDecision(input, "lead").concerns).toEqual([{ kind: "caution", text: "On-call one week in six" }]);
    expect(concernsOf(input).map((c) => c.text)).toEqual(["On-call one week in six", "Four-hour take-home", "Equity not stated"]);
  });

  it("puts what to check first before everything, unless the facts already say it", () => {
    const input = { why, caveats: ["On-call one week in six"], checkFirst: "The listing was last verified 4 days ago" };
    expect(selectDecision(input, "peer").concerns).toEqual([{ kind: "caution", text: "The listing was last verified 4 days ago", checkFirst: true }]);
    const repeat = { ...input, checkFirst: "The listing says remote but not where from", eligibilityHeadline: "The listing says remote but not where from." };
    expect(selectDecision(repeat, "peer").concerns[0]!.text).toBe("On-call one week in six");
  });

  it("puts a stated requirement the posting leaves open before a caveat; a mere omission after", () => {
    const unresolved = "Unresolved: you require at least USD 140,000 per year, and the posting doesn't say";
    expect(isStatedUnresolved(unresolved)).toBe(true);
    expect(isStatedUnresolved("Pay isn't published: unknown, not low")).toBe(false);
    const input = { why, caveats: ["Senior level, a step below your latest title"], unknowns: [unresolved] };
    expect(selectDecision(input, "lead").concerns).toEqual([{ kind: "unresolved", text: unresolved }]);
    const omission = { why, caveats: ["Senior level, a step below your latest title"], unknowns: ["Pay isn't published: unknown, not low"] };
    expect(concernsOf(omission).map((c) => c.kind)).toEqual(["caution", "missing"]);
  });

  it("keeps a stated unresolved requirement visible when a separate gate note comes first", () => {
    const unresolved = "Unresolved: you require remote roles open to Brazil, and the posting doesn't say where remote work is allowed";
    const input = {
      why,
      caveats: ["Senior level, a step below your latest title"],
      unknowns: [unresolved],
      checkFirst: "The employer's board could not be verified recently",
    };
    for (const variant of ["lead", "peer"] as const) {
      expect(selectDecision(input, variant).concerns).toEqual([
        { kind: "caution", text: input.checkFirst, checkFirst: true },
        { kind: "unresolved", text: unresolved },
      ]);
    }
  });

  it("never drops an unknown, and says a line once when it is both a caveat and an unknown", () => {
    const input = { why: [], caveats: ["Equity not stated"], unknowns: ["Equity not stated"] };
    expect(concernsOf(input)).toEqual([{ kind: "missing", text: "Equity not stated" }]);
    expect(selectDecision(input, "peer")).toEqual({ reasons: [], concerns: [{ kind: "missing", text: "Equity not stated" }] });
  });
});
