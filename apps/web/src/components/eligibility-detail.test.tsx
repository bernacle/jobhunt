import { render, screen, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { EligibilityDetail as Detail } from "@/lib/api-types";

import { violations } from "../../test/axe";
import { EligibilityDetail } from "./eligibility-detail";

const detail: Detail = {
  status: "conditional",
  headline: "Eligible if you can overlap four hours with San Francisco",
  disclaimer: "A compatibility signal from the posting and your profile, not legal advice about work authorization.",
  reasons: [
    {
      rule: "region_constraint",
      verdict: "pass",
      conclusion: "the listing is remote from anywhere",
      evidence: ["greenhouse:acme (locations): Remote - Worldwide"],
    },
    { rule: "timezone", verdict: "conditional", conclusion: "four hours of overlap with Pacific time", evidence: [] },
    { rule: "listing", verdict: "not_applicable", conclusion: "ignored" },
  ],
};

describe("EligibilityDetail", () => {
  it("shows each requirement with the posting's words, the verdict and why", async () => {
    const { container } = render(<EligibilityDetail detail={detail} />);
    const rows = screen.getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByText("Region")).toBeInTheDocument();
    expect(within(rows[0]!).getByText("greenhouse:acme (locations): Remote - Worldwide")).toBeInTheDocument();
    expect(rows[0]).toHaveTextContent("Fine · The listing is remote from anywhere");
    expect(within(rows[1]!).getByText("Not stated in the posting")).toBeInTheDocument();
    expect(rows[1]).toHaveTextContent("On a condition · Four hours of overlap with Pacific time");
    expect(await violations(container)).toEqual([]);
  });

  it("lays rows out by the width of its own column, not the viewport's", () => {
    const { container } = render(<EligibilityDetail detail={detail} />);
    // A container: three columns only where the container is wide enough.
    expect(container.firstElementChild).toHaveClass("@container");
    for (const row of screen.getAllByRole("listitem")) {
      expect(row.className).toContain("@xl:grid-cols-");
      expect(row.className).not.toMatch(/(^|\s)(sm|md|lg):grid-cols-/);
      // Stacked, the posting's words keep a visible label.
      expect(within(row).getByText("Posting says:")).toHaveClass("@xl:sr-only");
    }
  });
});
