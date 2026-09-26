import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DecisionBrief } from "./decision-brief";

describe("DecisionBrief", () => {
  it("lists why, then every caution and unknown, each marked for what it is", () => {
    render(
      <DecisionBrief
        why={["Platform roles: a role you want"]}
        caveats={["Timezone overlap required"]}
        unknowns={["Pay isn't published: unknown, not low", "Pay isn't published: unknown, not low"]}
      />,
    );
    expect(screen.getByText("Platform roles: a role you want")).toBeInTheDocument();
    const caveats = screen.getAllByRole("list")[1]!;
    expect(caveats.querySelectorAll("li")).toHaveLength(2);
    expect(screen.getByText("Timezone overlap required").closest("li")).toHaveTextContent(/^Caution:/);
    expect(screen.getByText("Pay isn't published: unknown, not low").closest("li")).toHaveTextContent(/^Not stated:/);
  });

  it("puts the ranking's check-first note before the rest, as a caution", () => {
    render(<DecisionBrief why={[]} caveats={["Far from your minimum"]} checkFirst="the listing was last verified 4 days ago" />);
    const items = screen.getAllByRole("list")[0]!.querySelectorAll("li");
    expect(items[0]).toHaveTextContent("Caution: Check first: The listing was last verified 4 days ago");
  });

  it("never hides that nothing was flagged or nothing is personal yet", () => {
    render(<DecisionBrief why={[]} caveats={[]} />);
    expect(screen.getByText("Nothing specific to you yet.")).toBeInTheDocument();
    expect(screen.getByText("Nothing flagged.")).toBeInTheDocument();
  });
});
