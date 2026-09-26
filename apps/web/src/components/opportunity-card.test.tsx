import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ActionResult } from "@/app/actions";
import type { FeedbackResult } from "@/lib/api-types";

import { feedItem, feedbackResult } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { OpportunityCard } from "./opportunity-card";

vi.mock("next/navigation", () => ({ useRouter: () => ({ refresh: vi.fn() }) }));

const now = new Date("2026-09-25T12:00:00Z");

function actions(result: ActionResult<FeedbackResult> = { ok: true, data: feedbackResult() }) {
  return {
    feedback: vi.fn(async () => result),
    putAside: vi.fn(async () => result),
  };
}

describe("OpportunityCard", () => {
  it("shows the opportunity, why, what to consider, then trust facts", async () => {
    const { container } = render(<OpportunityCard item={feedItem()} actions={actions()} now={now} />);
    expect(screen.getByRole("heading", { level: 2, name: "Senior Backend Engineer (Go)" })).toBeInTheDocument();
    expect(screen.getByText("Ledgerly")).toBeInTheDocument();
    expect(screen.getByText("Strong fit")).toBeInTheDocument();
    expect(screen.getByText("Why this may be worth your time")).toBeInTheDocument();
    expect(screen.getByText("Backend roles: a role you want")).toBeInTheDocument();
    expect(screen.getByText("Things to consider")).toBeInTheDocument();
    // Backend lines are shown as sentences.
    expect(screen.getByText("Senior level, a step below your latest title")).toBeInTheDocument();
    expect(screen.getByText(/USD 150,000 – 190,000 per year/)).toBeInTheDocument();
    expect(screen.getByText(/· verified/)).toBeInTheDocument();
    expect(screen.getByText(/You appear eligible/)).toBeInTheDocument();
    expect(screen.getByText("Verified 18 min ago on the employer's job board")).toBeInTheDocument();
    // No match percentage anywhere.
    expect(container.textContent).not.toMatch(/\d+\s?%/);
    expect(await violations(container)).toEqual([]);
  });

  it("says when pay is unknown, and never treats that as low", () => {
    render(
      <OpportunityCard
        item={feedItem({ compensation: { status: "not_published", ranges: [], verified: false } })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText("Pay not published")).toBeInTheDocument();
  });

  it("keeps listed-but-unverified pay distinct from verified pay", () => {
    render(
      <OpportunityCard
        item={feedItem({ compensation: { status: "published", ranges: ["EUR 90,000 per year"], verified: false } })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText("EUR 90,000 per year (as listed)")).toBeInTheDocument();
    expect(screen.queryByText(/· verified/)).not.toBeInTheDocument();
  });

  it("makes uncertain eligibility and unverified listings visible", () => {
    render(
      <OpportunityCard
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
    expect(screen.getByText(/Eligible on a condition — eligible if you relocate to Portugal/)).toBeInTheDocument();
    expect(screen.getByText("Check first:")).toBeInTheDocument();
    expect(screen.getByText(/Couldn't be verified recently: the employer's board didn't answer/)).toBeInTheDocument();
  });

  it("says what changed when a reviewed job comes back", () => {
    render(
      <OpportunityCard
        item={feedItem({ reason: "changed", changes: ["Pay is now published: USD 150,000 – 190,000 per year"], stage: "saved" })}
        actions={actions()}
        now={now}
      />,
    );
    expect(screen.getByText(/Changed since you looked:/)).toBeInTheDocument();
    expect(screen.getByText(/Pay is now published/)).toBeInTheDocument();
  });

  it("saves, then collapses to a confirmation once the API agrees", async () => {
    const a = actions();
    render(<OpportunityCard item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(a.feedback).toHaveBeenCalledWith("opp_0123456789abcdef0123456789abcdef", "save");
    expect(await screen.findByRole("status")).toHaveTextContent("Saved. It's in Applications.");
  });

  it("marks applied", async () => {
    const a = actions({ ok: true, data: feedbackResult({ action: "applied" }) });
    render(<OpportunityCard item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "I applied" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "applied");
    expect(await screen.findByRole("status")).toHaveTextContent("Marked as applied");
  });

  it("puts it aside without sending a negative signal", async () => {
    const a = actions();
    render(<OpportunityCard item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Not now" }));
    expect(a.putAside).toHaveBeenCalledOnce();
    expect(a.feedback).not.toHaveBeenCalled();
    expect(await screen.findByRole("status")).toHaveTextContent("Put aside");
  });

  it("asks why when rejecting, keeps free text, and offers suggestions", async () => {
    const a = actions({
      ok: true,
      data: feedbackResult({
        action: "reject",
        interpretation: { reason: "x", understood: true, read_as: ["avoid company: large companies"], reader: "rules/1" },
      }),
    });
    render(<OpportunityCard item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Not for me" }));
    const dialog = screen.getByRole("dialog", { name: "Why isn't this for you?" });
    const box = within(dialog).getByLabelText(/Your reason/);
    await userEvent.type(box, "on-call every other week");
    await userEvent.click(within(dialog).getByRole("button", { name: "Too corporate" }));
    expect(box).toHaveValue("on-call every other week; too corporate");
    expect(await violations(dialog)).toEqual([]);
    await userEvent.click(within(dialog).getByRole("button", { name: "Not for me" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "reject", "on-call every other week; too corporate");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Won't be recommended again. JobHunt read your reason as: avoid company: large companies.",
    );
  });

  it("rejecting without a reason is allowed", async () => {
    const a = actions();
    render(<OpportunityCard item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Not for me" }));
    const dialog = screen.getByRole("dialog");
    await userEvent.click(within(dialog).getByRole("button", { name: "Not for me" }));
    expect(a.feedback).toHaveBeenCalledWith(expect.any(String), "reject", "");
  });

  it("rolls back and explains when the API refuses", async () => {
    const a = actions({ ok: false, code: "conflict", title: "Something changed at the same time", message: "Try again." });
    render(<OpportunityCard item={feedItem()} actions={a} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Something changed at the same time. Try again.");
    // The card is back, actions enabled.
    await waitFor(() => expect(screen.getByRole("button", { name: "Save" })).toBeEnabled());
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });
});
