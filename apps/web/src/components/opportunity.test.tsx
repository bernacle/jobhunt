import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ActionResult } from "@/app/actions";
import type { FeedbackResult } from "@/lib/api-types";

import { feedItem, feedbackResult } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { OpportunityLead, OpportunityPeer } from "./opportunity";

vi.mock("next/navigation", () => ({ useRouter: () => ({ refresh: vi.fn() }) }));

const now = new Date("2026-09-25T12:00:00Z");

function actions(result: ActionResult<FeedbackResult> = { ok: true, data: feedbackResult() }) {
  return {
    feedback: vi.fn(async () => result),
    putAside: vi.fn(async () => result),
  };
}

describe("OpportunityLead", () => {
  it("shows the opportunity, its facts, why, what to consider, then verification", async () => {
    const { container } = render(<OpportunityLead item={feedItem()} actions={actions()} now={now} />);
    expect(screen.getByRole("heading", { level: 2, name: "Senior Backend Engineer (Go)" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Senior Backend Engineer (Go)" })).toHaveAttribute(
      "href",
      "/opportunities/opp_0123456789abcdef0123456789abcdef",
    );
    expect(screen.getByText("Ledgerly")).toBeInTheDocument();
    expect(screen.getByText("Strong fit")).toBeInTheDocument();
    expect(screen.getByText("Why it may be worth your time")).toBeInTheDocument();
    expect(screen.getByText("Backend roles: a role you want")).toBeInTheDocument();
    expect(screen.getByText("Things to consider")).toBeInTheDocument();
    // Backend lines are shown as sentences; caveats are cautions.
    expect(screen.getByText("Senior level, a step below your latest title").closest("li")).toHaveTextContent(/^Caution:/);
    // Verified pay carries the check (and says so to screen readers).
    expect(screen.getByText(/USD 150,000 – 190,000 per year/)).toHaveTextContent("USD 150,000 – 190,000 per year ✓ verified");
    expect(screen.getByText("Eligible")).toBeInTheDocument();
    expect(screen.getByText(/the listing is remote from anywhere/)).toBeInTheDocument();
    expect(screen.getByText("Verified 18 min ago on the employer's job board")).toBeInTheDocument();
    // The four actions, Save last and primary.
    const buttons = screen.getAllByRole("button").map((b) => b.textContent);
    expect(buttons).toEqual(["Not now", "Not for me", "I applied", "Save"]);
    // No match percentage, no score, anywhere.
    expect(container.textContent).not.toMatch(/\d+\s?%|match score|good fit|stretch/i);
    expect(await violations(container)).toEqual([]);
  });

  it("marks unknowns as not stated, apart from cautions", () => {
    const item = feedItem({
      consider: ["Requires 4 hours' overlap with Pacific time", "Equity not stated"],
      unknowns: ["Equity not stated"],
    });
    render(<OpportunityLead item={item} actions={actions()} now={now} />);
    expect(screen.getByText("Requires 4 hours' overlap with Pacific time").closest("li")).toHaveTextContent(/^Caution:/);
    expect(screen.getByText("Equity not stated").closest("li")).toHaveTextContent(/^Not stated:/);
  });

  it("says when pay is unknown, and never treats that as low", () => {
    render(<OpportunityLead item={feedItem({ compensation: { status: "not_published", ranges: [], verified: false } })} actions={actions()} now={now} />);
    expect(screen.getByText("Pay not published")).toBeInTheDocument();
  });

  it("keeps listed-but-unverified pay distinct from verified pay", () => {
    render(
      <OpportunityLead
        item={feedItem({ compensation: { status: "published", ranges: ["EUR 90,000 per year"], verified: false } })}
        actions={actions()}
        now={now}
      />,
    );
    // Listed pay: secondary ink, no check.
    expect(screen.getByText("EUR 90,000 per year (as listed)")).toHaveClass("text-fg-secondary");
    expect(screen.getByText("EUR 90,000 per year (as listed)").textContent).not.toContain("✓");
  });

  it("never presents remote without a scope as resolved", () => {
    render(<OpportunityLead item={feedItem({ locations: ["Remote"], workplace: "remote" })} actions={actions()} now={now} />);
    expect(screen.getByText("Remote · region not stated")).toHaveClass("nr-inferred");
  });

  it("makes conditional eligibility and unverified listings visible", () => {
    render(
      <OpportunityLead
        item={feedItem({
          eligibility: { status: "conditional", headline: "Eligible if you relocate to Portugal" },
          recommendation: "verify_first",
          recommendation_note: "The listing was last verified 4 days ago",
          verification: { state: "could_not_verify", trusted: false, not_trusted_because: "The employer's board didn't answer" },
        })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText("Eligible on a condition")).toHaveClass("nr-inferred");
    expect(screen.getByText(/eligible if you relocate to Portugal/)).toBeInTheDocument();
    expect(screen.getByText("Check first:")).toBeInTheDocument();
    expect(screen.getByText(/Couldn't be verified recently: the employer's board didn't answer/)).toBeInTheDocument();
    // No check mark on a listing that isn't verified.
    expect(screen.getByText(/Couldn't be verified recently/).textContent).not.toContain("✓");
  });

  it("says what changed when a reviewed job comes back", () => {
    render(
      <OpportunityLead
        item={feedItem({ reason: "changed", changes: ["Pay is now published: USD 150,000 – 190,000 per year"], stage: "saved" })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText(/Changed since you looked:/)).toBeInTheDocument();
    expect(screen.getByText(/Pay is now published/)).toBeInTheDocument();
  });

  it("saves, then folds into one line once the API agrees", async () => {
    const a = actions();
    render(<OpportunityLead item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(a.feedback).toHaveBeenCalledWith("opp_0123456789abcdef0123456789abcdef", "save");
    expect(await screen.findByRole("status")).toHaveTextContent("Saved. It's in Applications.");
    expect(screen.getByRole("heading", { level: 2 })).toHaveTextContent("Senior Backend Engineer (Go) · Ledgerly");
  });

  it("marks applied", async () => {
    const a = actions({ ok: true, data: feedbackResult({ action: "applied" }) });
    render(<OpportunityLead item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "I applied" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "applied");
    expect(await screen.findByRole("status")).toHaveTextContent("Marked as applied");
  });

  it("Not now puts it aside without sending a negative signal", async () => {
    const a = actions();
    render(<OpportunityLead item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(a.putAside).toHaveBeenCalledOnce();
    expect(a.feedback).not.toHaveBeenCalled();
    expect(await screen.findByRole("status")).toHaveTextContent("Put aside. It comes back only if it changes in a way that matters. Nothing was learned from it.");
  });

  it("Not for me asks why, keeps free text, offers suggestions, and teaches", async () => {
    const a = actions({
      ok: true,
      data: feedbackResult({
        action: "reject",
        interpretation: { reason: "x", understood: true, read_as: ["avoid company: large companies"], reader: "rules/1" },
      }),
    });
    render(<OpportunityLead item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Not for me" }));
    const dialog = screen.getByRole("dialog", { name: "Not for me" });
    expect(dialog).toHaveAccessibleDescription(/Senior Backend Engineer \(Go\) · Ledgerly won't be recommended again/);
    // The dialog points to Not now for someone who only wants it gone today.
    expect(within(dialog).getByText(/Use Not now instead/)).toBeInTheDocument();
    const box = within(dialog).getByLabelText(/What didn't fit/);
    await userEvent.type(box, "on-call every other week");
    await userEvent.click(within(dialog).getByRole("button", { name: "Too corporate" }));
    expect(box).toHaveValue("on-call every other week; too corporate");
    expect(await violations(dialog)).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Mark not for me" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "reject", "on-call every other week; too corporate");
    expect(a.putAside).not.toHaveBeenCalled();
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Won't be recommended again. Narrow read your reason as: avoid company: large companies.",
    );
  });

  it("rejecting without a reason is allowed", async () => {
    const a = actions();
    render(<OpportunityLead item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Not for me" }));
    await userEvent.click(within(screen.getByRole("dialog")).getByRole("button", { name: "Mark not for me" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "reject", "");
  });

  it("rolls back and explains when the API refuses", async () => {
    const a = actions({ ok: false, code: "conflict", title: "Something changed at the same time", message: "Try again." });
    render(<OpportunityLead item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Something changed at the same time. Try again.");
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});

describe("OpportunityPeer", () => {
  it("is denser but keeps the facts, reasons, cautions, verification and all four actions", async () => {
    const { container } = render(<OpportunityPeer item={feedItem({ tier: "worth_reviewing" })} actions={actions()} now={now} />);
    expect(screen.getByRole("heading", { level: 2, name: "Senior Backend Engineer (Go)" })).toBeInTheDocument();
    expect(screen.getByText("Worth reviewing")).toBeInTheDocument();
    // Labels are for screen readers only on peers.
    expect(screen.getByText("Why it may be worth your time")).toHaveClass("sr-only");
    expect(screen.getByText("Backend roles: a role you want")).toBeInTheDocument();
    expect(screen.getByText(/USD 150,000 – 190,000 per year/)).toBeInTheDocument();
    expect(screen.getByText("Verified 18 min ago on the employer's job board")).toBeInTheDocument();
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Not now", "Not for me", "I applied", "Save"]);
    expect(container.textContent).not.toMatch(/\d+\s?%/);
    expect(await violations(container)).toEqual([]);
  });
});
