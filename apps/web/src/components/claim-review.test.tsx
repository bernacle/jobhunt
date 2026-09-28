import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { UnresolvedClaim } from "@/lib/api-types";

import { violations } from "../../test/axe";
import { ClaimReview } from "./claim-review";

const claims: UnresolvedClaim[] = [
  {
    id: "clm_1",
    kind: "domain",
    text: "Worked in payments at Acme Payments",
    why: "inferred by Narrow; confirm or reject",
    about: "Staff Software Engineer at Acme Payments",
    about_id: "exp_1",
    provenance: "inferred",
    confidence: "medium",
    snippet: "Acme Payments",
    section: "Experience",
    document: "ana_lima.md",
    basis: "mentions “Payments”, “settlement”",
  },
  {
    id: "clm_2",
    kind: "technology",
    text: "Used Kafka",
    why: "read from your resume, but the reading is uncertain",
    provenance: "extracted",
    confidence: "low",
    snippet: "Tech: Go, PostgreSQL, Kafka",
  },
];

const ok = { ok: true as const, data: { decided: [], needs_review_total: 1 } };

describe("ClaimReview", () => {
  it("groups claims by what they're about, with actions beside each claim and the evidence on demand", async () => {
    const { container } = render(<ClaimReview claims={claims} total={2} decide={vi.fn(async () => ok)} />);
    expect(screen.getByText("2 claims need your review.")).toBeInTheDocument();
    const group = screen.getByRole("region", { name: "Staff Software Engineer at Acme Payments" });
    expect(within(group).getByText("inferred by Narrow")).toBeInTheDocument();
    const row = within(group).getByText("Worked in payments at Acme Payments").closest("li")!;
    expect(within(row).getByRole("button", { name: "Confirm: Worked in payments at Acme Payments" })).toBeInTheDocument();
    expect(within(row).getByRole("button", { name: "Reject: Worked in payments at Acme Payments" })).toBeInTheDocument();
    // The quote and where it's from aren't between the claim and its actions.
    expect(within(row).queryByText("Acme Payments", { exact: true })).not.toBeInTheDocument();
    await userEvent.click(within(row).getByRole("button", { name: /^See evidence/ }));
    const panel = screen.getByRole("dialog", { name: "Worked in payments at Acme Payments" });
    expect(within(panel).getByText("Acme Payments")).toBeInTheDocument();
    expect(within(panel).getByText("ana_lima.md · Experience")).toBeInTheDocument();
    expect(within(panel).getByText(/Inferred by Narrow; confirm or reject/)).toBeInTheDocument();
    expect(within(panel).getByText("mentions “Payments”, “settlement”")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
    // Profile-wide claims have their own group.
    expect(screen.getByRole("region", { name: "Your profile in general" })).toHaveTextContent("read from your resume");
  });

  it("confirms and rejects (with an optional note) through the API", async () => {
    const decide = vi.fn(async () => ok);
    render(<ClaimReview claims={claims} total={2} decide={decide} />);
    await userEvent.click(screen.getByRole("button", { name: "Confirm: Worked in payments at Acme Payments" }));
    expect(decide).toHaveBeenCalledWith("clm_1", "confirm", undefined);
    expect(await screen.findByText(/Confirmed: it can be used as evidence/)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Reject: Used Kafka" }));
    await userEvent.type(screen.getByLabelText(/Why is it wrong/), "never used it");
    await userEvent.click(screen.getByRole("button", { name: "Reject claim" }));
    expect(decide).toHaveBeenLastCalledWith("clm_2", "reject", "never used it");
    expect(await screen.findByText("Nothing left to review.")).toBeInTheDocument();
  });
});
