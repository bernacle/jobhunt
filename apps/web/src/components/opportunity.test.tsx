import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ActionResult } from "@/app/actions";
import type { FeedbackResult } from "@/lib/api-types";

import { feedItem, feedbackResult } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { OpportunityLead, OpportunityPeer, TodayFeed } from "./opportunity";

vi.mock("next/navigation", () => ({ useRouter: () => ({ refresh: vi.fn() }) }));

const now = new Date("2026-09-25T12:00:00Z");

/** The four decisions, in order, leaving out "See evidence". */
function decisions(scope: HTMLElement = document.body) {
  return within(scope)
    .getAllByRole("button")
    .map((b) => b.textContent)
    .filter((t) => !t?.startsWith("See evidence"));
}

function actions(result: ActionResult<FeedbackResult> = { ok: true, data: feedbackResult() }) {
  return {
    feedback: vi.fn(async () => result),
    putAside: vi.fn(async () => result),
  };
}

describe("OpportunityLead", () => {
  it("shows the decision: facts, the two strongest reasons, one concern, verification, then actions", async () => {
    const { container } = render(<OpportunityLead item={feedItem()} actions={actions()} now={now} />);
    expect(screen.getByRole("heading", { level: 2, name: "Senior Backend Engineer (Go)" })).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Senior Backend Engineer (Go)" })).toHaveAttribute(
      "href",
      "/opportunities/opp_0123456789abcdef0123456789abcdef",
    );
    expect(screen.getByText("Ledgerly")).toBeInTheDocument();
    expect(screen.getByText("Strong fit")).toBeInTheDocument();
    expect(screen.getByText("Backend roles: a role you want").closest("li")).toHaveTextContent(/^Reason:/);
    expect(screen.getByText("Small teams: a kind of company or team you want")).toBeInTheDocument();
    // Backend lines are shown as sentences; the strongest caveat is the one concern shown.
    expect(screen.getByText("Senior level, a step below your latest title").closest("li")).toHaveTextContent(/^Caution:/);
    expect(screen.queryByText("The posting doesn't say whether it's product companies")).not.toBeInTheDocument();
    // Verified pay carries the check (and says so to screen readers).
    expect(screen.getByText(/USD 150,000 – 190,000 per year/)).toHaveTextContent("USD 150,000 – 190,000 per year ✓ verified");
    expect(screen.getByText("Eligible")).toBeInTheDocument();
    expect(screen.getByText(/the listing is remote from anywhere/)).toBeInTheDocument();
    expect(screen.getByText("Verified 18 min ago on the employer's job board")).toBeInTheDocument();
    // The four actions, Save last and primary.
    expect(decisions()).toEqual(["Not now", "Not for me", "I applied", "Save"]);
    // No match percentage, no score, anywhere.
    expect(container.textContent).not.toMatch(/\d+\s?%|match score|good fit|stretch/i);
    expect(await violations(container)).toEqual([]);
  });

  it("keeps everything else one labelled action away, in an evidence panel", async () => {
    render(<OpportunityLead item={feedItem({ sources: 3 })} actions={actions()} now={now} />);
    const trigger = screen.getByRole("button", { name: /^See evidence for Senior Backend Engineer \(Go\) at Ledgerly/ });
    await userEvent.click(trigger);
    const panel = screen.getByRole("dialog", { name: "Why Narrow surfaced it" });
    expect(within(panel).getByText("The posting doesn't say whether it's product companies").closest("li")).toHaveTextContent(/^Caution:/);
    expect(within(panel).getByText("3 boards")).toBeInTheDocument();
    expect(within(panel).getByText(/Greenhouse · ledgerly/)).toBeInTheDocument();
    expect(within(panel).getByRole("link", { name: "Open the full brief" })).toHaveAttribute("href", "/opportunities/opp_0123456789abcdef0123456789abcdef");
    expect(await violations(panel)).toEqual([]);
    await userEvent.click(within(panel).getByRole("button", { name: "Close" }));
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(trigger).toHaveFocus();
  });

  it("marks unknowns as not stated, apart from cautions", async () => {
    const item = feedItem({
      consider: ["Requires 4 hours' overlap with Pacific time", "Equity not stated"],
      unknowns: ["Equity not stated"],
    });
    render(<OpportunityLead item={item} actions={actions()} now={now} />);
    expect(screen.getByText("Requires 4 hours' overlap with Pacific time").closest("li")).toHaveTextContent(/^Caution:/);
    await userEvent.click(screen.getByRole("button", { name: /^See evidence/ }));
    const panel = screen.getByRole("dialog");
    expect(within(panel).getByText("Equity not stated").closest("li")).toHaveTextContent(/^Not stated:/);
  });

  it("puts a requirement the posting leaves unresolved before a soft caveat, and never drops it", async () => {
    const item = feedItem({
      compensation: { status: "not_published", ranges: [], verified: false },
      consider: ["Senior level, a step below your latest title", "Unresolved: you require at least USD 140,000 per year, and this job's pay can't be checked against it"],
      unknowns: ["Unresolved: you require at least USD 140,000 per year, and this job's pay can't be checked against it"],
    });
    render(<OpportunityLead item={item} actions={actions()} now={now} />);
    expect(screen.getByText(/^Unresolved: you require at least USD 140,000/)).toBeInTheDocument();
    expect(screen.queryByText("Senior level, a step below your latest title")).not.toBeInTheDocument();
    // Unknown pay stays unknown in the facts.
    expect(screen.getByText("Pay not published")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: /^See evidence/ }));
    expect(within(screen.getByRole("dialog")).getByText("Senior level, a step below your latest title")).toBeInTheDocument();
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

  it("makes conditional eligibility and unverified listings visible", async () => {
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
    expect(screen.getByText("Check first:").closest("li")).toHaveTextContent("Caution: Check first: The listing was last verified 4 days ago");
    // No check mark on a listing that isn't verified; the reason is in the evidence.
    expect(screen.getByText("Couldn't be verified recently").textContent).not.toContain("✓");
    await userEvent.click(screen.getByRole("button", { name: /^See evidence/ }));
    expect(within(screen.getByRole("dialog")).getByText(/Couldn't be verified recently: the employer's board didn't answer/)).toBeInTheDocument();
  });

  it("doesn't repeat an unclear eligibility as a check-first note: the facts already say it", () => {
    render(
      <OpportunityLead
        item={feedItem({
          eligibility: { status: "uncertain", headline: "The listing says remote but not where from" },
          recommendation: "eligibility_unclear" as never,
          recommendation_note: "The listing says remote but not where from",
        })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText("Eligibility unclear")).toHaveClass("nr-inferred");
    expect(screen.getAllByText(/the listing says remote but not where from/i)).toHaveLength(1);
    expect(screen.queryByText("Check first:")).not.toBeInTheDocument();
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
    await userEvent.click(within(dialog).getByRole("button", { name: "Company too big" }));
    expect(box).toHaveValue("on-call every other week; company too big");
    expect(await violations(dialog)).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Mark not for me" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "reject", "on-call every other week; company too big");
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
  it("is a comparison object: facts, one reason, one concern, verification and all four actions", async () => {
    const { container } = render(<OpportunityPeer item={feedItem({ tier: "worth_reviewing" })} actions={actions()} now={now} />);
    expect(screen.getByRole("heading", { level: 2, name: "Senior Backend Engineer (Go)" })).toBeInTheDocument();
    expect(screen.getByText("Worth reviewing")).toBeInTheDocument();
    expect(screen.getByText("Why it may be worth your time:")).toHaveClass("sr-only");
    expect(screen.getByText("Backend roles: a role you want")).toBeInTheDocument();
    expect(screen.queryByText("Small teams: a kind of company or team you want")).not.toBeInTheDocument();
    expect(screen.getByText("Senior level, a step below your latest title").closest("li")).toHaveTextContent(/^Caution:/);
    expect(screen.queryByText("The posting doesn't say whether it's product companies")).not.toBeInTheDocument();
    expect(screen.getByText(/USD 150,000 – 190,000 per year/)).toBeInTheDocument();
    expect(screen.getByText("Remote - Worldwide")).toBeInTheDocument();
    expect(screen.getByText("Eligible")).toBeInTheDocument();
    expect(screen.getByText("Verified 18 min ago")).toBeInTheDocument();
    expect(decisions()).toEqual(["Not now", "Not for me", "I applied", "Save"]);
    expect(container.textContent).not.toMatch(/\d+\s?%/);
    expect(await violations(container)).toEqual([]);
  });

  it("keeps unknown pay and unclear eligibility in its facts, never hidden to shorten it", () => {
    render(
      <OpportunityPeer
        item={feedItem({
          compensation: { status: "not_published", ranges: [], verified: false },
          eligibility: { status: "uncertain", headline: "The listing says remote but not where from" },
          locations: ["Remote"],
        })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText("Pay not published")).toBeInTheDocument();
    expect(screen.getByText("Remote · region not stated")).toHaveClass("nr-inferred");
    expect(screen.getByText("Eligibility unclear")).toHaveClass("nr-inferred");
    expect(screen.getByText(/the listing says remote but not where from/)).toBeInTheDocument();
  });

  it("leaves no filler when there is nothing to flag", () => {
    const { container } = render(<OpportunityPeer item={feedItem({ consider: [] })} actions={actions()} now={now} />);
    expect(container.textContent).not.toMatch(/Nothing flagged|No concerns/);
  });
});

describe("TodayFeed", () => {
  const a = feedItem({ id: "opp_a", title: "Staff Engineer A", company: "Acme" });
  const b = feedItem({ id: "opp_b", title: "Backend Engineer B", company: "Beta" });
  const c = feedItem({ id: "opp_c", title: "Platform Engineer C", company: "Cove" });

  it("keeps each lead's outcome with its own opportunity when a refresh picks a different lead", async () => {
    const act = actions();
    const { rerender } = render(<TodayFeed items={[a, b]} actions={act} now={now} />);
    const list = screen.getByRole("list", { name: "Recommendations" });
    await userEvent.click(within(within(list).getAllByRole("article")[0]!).getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Saved");

    // A refreshed feed: A is gone, B now leads.
    rerender(<TodayFeed items={[b, c]} actions={act} now={now} />);
    const lead = screen.getAllByRole("article")[0]!;
    expect(within(lead).getByRole("heading", { level: 2, name: "Backend Engineer B" })).toBeInTheDocument();
    // B is undecided: its actions are there, and A's "Saved" didn't carry over.
    expect(within(lead).getByRole("button", { name: "Save" })).toBeEnabled();
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
    expect(screen.queryByText(/Staff Engineer A/)).not.toBeInTheDocument();
  });
});

describe("MoreAtCompany", () => {
  it("names the company's other roles instead of giving them Today slots", async () => {
    const item = feedItem({
      company: "Supabase",
      also_at_company: [
        { id: "opp_1", title: "OrioleDB Deployment Engineer (AMER)" },
        { id: "opp_2", title: "Software Engineer - Branching" },
        { id: "opp_3", title: "Engineering Productivity Engineer" },
      ],
    });
    render(<OpportunityLead item={item} actions={actions()} now={now} />);
    await userEvent.click(screen.getByText("+3 more roles at Supabase"));
    expect(screen.getByRole("link", { name: "Software Engineer - Branching" })).toHaveAttribute("href", "/opportunities/opp_2");
  });

  it("says nothing when the company has no other role", () => {
    render(<OpportunityLead item={feedItem()} actions={actions()} now={now} />);
    expect(screen.queryByText(/more roles? at/)).not.toBeInTheDocument();
  });
});
