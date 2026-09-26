import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { DecisionBrief } from "./decision-brief";

describe("DecisionBrief", () => {
  it("lists why, then every caveat and unknown", () => {
    render(
      <DecisionBrief
        why={["Platform roles: a role you want"]}
        consider={["Timezone overlap required"]}
        unknowns={["Pay isn't published: unknown, not low", "Timezone overlap required"]}
      />,
    );
    expect(screen.getByText("Platform roles: a role you want")).toBeInTheDocument();
    const caveats = screen.getAllByRole("list")[1]!;
    expect(caveats.querySelectorAll("li")).toHaveLength(2);
    expect(screen.getByText("Pay isn't published: unknown, not low")).toBeInTheDocument();
  });

  it("never hides that nothing was flagged or nothing is personal yet", () => {
    render(<DecisionBrief why={[]} consider={[]} />);
    expect(screen.getByText("Nothing specific to you yet.")).toBeInTheDocument();
    expect(screen.getByText("Nothing flagged.")).toBeInTheDocument();
  });
});
