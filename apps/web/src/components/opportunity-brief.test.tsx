import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { ActionResult } from "@/app/actions";
import type { FeedbackResult, JobDetail } from "@/lib/api-types";

import { feedbackResult, jobDetail } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { OpportunityBrief } from "./opportunity-brief";

vi.mock("next/navigation", () => ({ useRouter: () => ({ refresh: vi.fn() }) }));

const now = new Date("2026-09-25T12:00:00Z");

function actions(result: ActionResult<FeedbackResult> = { ok: true, data: feedbackResult() }) {
  return { feedback: vi.fn(async () => result), putAside: vi.fn(async () => result) };
}

function brief(job: JobDetail = jobDetail()) {
  return render(<OpportunityBrief job={job} context={null} actions={actions()} now={now} />);
}

/** A key fact's row ("Pay", "Eligibility"), as on Preferences. */
function fact(name: string) {
  return screen.getByRole("group", { name });
}

/** What the page shows before anything is opened. */
function visibleText() {
  return document.body.textContent ?? "";
}

function count(text: string) {
  return visibleText().split(text).length - 1;
}

describe("the decision first", () => {
  it("says what it is, the verdict, then the facts, then what to do, then the rest", async () => {
    const { container } = brief();
    const order = [
      screen.getByRole("heading", { level: 1, name: "Senior Backend Engineer (Go)" }),
      screen.getByText("A strong fit for what you want."),
      screen.getByText("Backend roles: a role you want"),
      fact("Pay"),
      fact("Location"),
      fact("Eligibility"),
      fact("Listing"),
      screen.getByRole("button", { name: "Save" }),
      screen.getByRole("region", { name: "Description" }),
    ];
    for (let i = 1; i < order.length; i++) {
      expect(order[i - 1]!.compareDocumentPosition(order[i]!) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    }
    expect(screen.getByText("Strong fit")).toBeInTheDocument();
    // No match percentage, no score.
    expect(container.textContent).not.toMatch(/\d+\s?%|match score/i);
    expect(await violations(container)).toEqual([]);
  });

  it("shows the two strongest distinct reasons and the most material concern; the rest is in the full reasoning", async () => {
    brief();
    expect(screen.getByText("Backend roles: a role you want")).toBeInTheDocument();
    expect(screen.getByText("Small teams: a kind of company or team you want")).toBeInTheDocument();
    expect(screen.queryByText("Go: in your recent work")).not.toBeInTheDocument();
    expect(screen.getByText("Senior level, a step below your latest title").closest("li")).toHaveTextContent(/^Caution:/);
    expect(screen.queryByText("The posting doesn't say whether it's product companies")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Full reasoning" }));
    const panel = screen.getByRole("dialog", { name: "Why Narrow surfaced it" });
    expect(within(panel).getByText("Go: in your recent work")).toBeInTheDocument();
    expect(within(panel).getByText("Payments: a domain you know")).toBeInTheDocument();
    expect(within(panel).getByText("The posting doesn't say whether it's product companies")).toBeInTheDocument();
    expect(within(panel).getByText(/Narrow read it as backend · senior · Go/)).toBeInTheDocument();
  });

  it("keeps a stated requirement the posting leaves open in view, never behind the reasoning", () => {
    brief(
      jobDetail({
        decision: {
          ...jobDetail().decision!,
          tier: "worth_reviewing",
          verdict: "worth reviewing once you know the pay.",
          caveats: ["senior level, a step below your latest title"],
          unknowns: ["Unresolved: you require at least USD 140,000 per year; the posting doesn't publish pay"],
        },
      }),
    );
    const concern = screen.getByText(/Unresolved: you require at least USD 140,000 per year/);
    expect(concern.closest("li")).toBeInTheDocument();
    expect(screen.queryByText("Senior level, a step below your latest title")).not.toBeInTheDocument();
  });

  it("flags nothing when there is nothing to flag", async () => {
    brief(jobDetail({ decision: { ...jobDetail().decision!, caveats: [], unknowns: [] } }));
    expect(screen.queryByText(/^(Caution|Not stated):/)).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Full reasoning" }));
    expect(within(screen.getByRole("dialog")).getByText("Nothing flagged.")).toBeInTheDocument();
  });
});

describe("key facts", () => {
  it("says each fact once, with how far it is confirmed", () => {
    brief();
    expect(within(fact("Pay")).getByText("USD 150,000 – 190,000 per year")).toBeInTheDocument();
    expect(within(fact("Pay")).getByText(/confirmed at the source 18 min ago/)).toBeInTheDocument();
    expect(within(fact("Location")).getByText("Remote - Worldwide")).toBeInTheDocument();
    expect(within(fact("Location")).getByText("Full time")).toBeInTheDocument();
    expect(within(fact("Eligibility")).getByText("Eligible")).toBeInTheDocument();
    expect(within(fact("Listing")).getByText("Verified 18 min ago on the employer's job board")).toBeInTheDocument();
    // Not repeated in a hero line, a section and the evidence.
    expect(count("USD 150,000 – 190,000 per year")).toBe(1);
    expect(count("the listing is remote from anywhere")).toBe(1);
    expect(count("Verified 18 min ago")).toBe(1);
  });

  it("says unknown pay and an unscoped Remote plainly, as unknown, not as bad", () => {
    brief(
      jobDetail({
        compensation: { status: "not_published", ranges: [], verified: false },
        locations: ["Remote"],
        eligibility: { ...jobDetail().eligibility!, status: "uncertain", headline: "The listing doesn't say where remote work is open to", option: null },
      }),
    );
    expect(within(fact("Pay")).getByText("Not published")).toBeInTheDocument();
    expect(within(fact("Pay")).getByText("Unknown, not low")).toBeInTheDocument();
    expect(within(fact("Location")).getByText("Remote · region not stated")).toHaveClass("nr-inferred");
    expect(within(fact("Eligibility")).getByText("Eligibility unclear")).toHaveClass("nr-inferred");
    expect(visibleText()).not.toMatch(/ineligible|Probably not eligible/i);
  });

  it("keeps pay in another currency as published, and every published figure one action away", async () => {
    brief(
      jobDetail({
        compensation: {
          status: "published",
          ranges: ["EUR 70,000 – 85,000 per year", "EUR 6,000 – 7,000 per month"],
          summary: "€70–85k base, or a monthly contract rate",
          verified: false,
        },
      }),
    );
    expect(within(fact("Pay")).getByText("EUR 70,000 – 85,000 per year")).toBeInTheDocument();
    expect(within(fact("Pay")).getByText("as listed, not confirmed at the source")).toBeInTheDocument();
    expect(screen.queryByText("EUR 6,000 – 7,000 per month")).not.toBeInTheDocument();
    await userEvent.click(within(fact("Pay")).getByRole("button", { name: "As published pay" }));
    const panel = screen.getByRole("dialog", { name: "Pay as published" });
    expect(within(panel).getByText("EUR 6,000 – 7,000 per month")).toBeInTheDocument();
    expect(within(panel).getByText("€70–85k base, or a monthly contract rate")).toBeInTheDocument();
    expect(within(panel).getByText(/never converts currencies or periods/)).toBeInTheDocument();
  });

  it("names the first locations and keeps the rest one action away", async () => {
    const places = ["Lisbon, Portugal", "Madrid, Spain", "Berlin, Germany", "Amsterdam, Netherlands", "Remote - EU"];
    brief(jobDetail({ locations: places, workplace: "hybrid" }));
    expect(within(fact("Location")).getByText("Lisbon, Portugal · Madrid, Spain · Berlin, Germany")).toBeInTheDocument();
    expect(within(fact("Location")).getByText("Hybrid · Full time")).toBeInTheDocument();
    await userEvent.click(within(fact("Location")).getByRole("button", { name: "All locations" }));
    expect(within(screen.getByRole("dialog")).getAllByRole("listitem").map((l) => l.textContent)).toEqual(places);
  });
});

describe("verification", () => {
  it("says a stale listing once, in words, without the check", () => {
    brief(
      jobDetail({
        verification: {
          state: "verified_active",
          trusted: false,
          verified_at: "2026-09-19T12:00:00Z",
          freshness: "stale",
          authority: "employer_configured_ats",
          source: "greenhouse:ledgerly",
          not_trusted_because: "not verified in the last 72 hours",
        },
      }),
    );
    const listing = within(fact("Listing"));
    expect(listing.getByText(/Last verified 6 days ago; not verified in the last 72 hours/)).toBeInTheDocument();
    expect(fact("Listing")).not.toHaveTextContent("✓");
    expect(count("Last verified")).toBe(1);
  });

  it("says a closed listing plainly", () => {
    brief(jobDetail({ verification: { state: "verified_closed", trusted: true, verified_at: "2026-09-25T11:00:00Z" } }));
    expect(within(fact("Listing")).getByText("The listing appears closed")).toBeInTheDocument();
  });
});

describe("evidence on demand", () => {
  it("opens the eligibility checks and the sources in a panel, and nothing before", async () => {
    brief();
    expect(screen.queryByText(/A compatibility signal/)).not.toBeInTheDocument();
    await userEvent.click(within(fact("Eligibility")).getByRole("button", { name: "Checks for eligibility" }));
    let panel = screen.getByRole("dialog", { name: "Eligibility checks" });
    expect(within(panel).getByText(/A compatibility signal/)).toBeInTheDocument();
    expect(within(panel).getByText("Remote - Worldwide")).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(within(fact("Eligibility")).getByRole("button", { name: "Checks for eligibility" })).toHaveFocus();

    await userEvent.click(within(fact("Listing")).getByRole("button", { name: "Sources of the listing" }));
    panel = screen.getByRole("dialog", { name: "Where it's listed" });
    expect(within(panel).getByRole("link", { name: /^Greenhouse · ledgerly/ })).toHaveAttribute("href", "https://job-boards.greenhouse.io/ledgerly/jobs/7001001");
    expect(within(panel).getByText("Pay confirmed at the source")).toBeInTheDocument();
    expect(within(panel).getByText("1 source")).toBeInTheDocument();
  });
});

describe("hard content", () => {
  it("keeps a very long title and company whole", () => {
    const title = "Principal Staff Distributed Systems Backend Engineer, Real-Time Cross-Border Payments Reconciliation Platform (Go/Rust)";
    const company = "Northwind Ledger Financial Infrastructure & Developer Tooling Holdings International";
    brief(jobDetail({ title, company }));
    expect(screen.getByRole("heading", { level: 1, name: title })).toHaveClass("[overflow-wrap:anywhere]");
    expect(screen.getByText(company, { selector: "li" })).toHaveClass("truncate");
  });

  it("works for a sparse listing with no profile: no brief, nothing checked, said plainly", async () => {
    const { container } = brief(
      jobDetail({
        decision: null,
        eligibility: null,
        compensation: { status: "unknown", ranges: [], verified: false },
        locations: [],
        workplace: null,
        employment: null,
        department: null,
        description: "",
        posted_at: null,
        sources: [],
        verification: { state: "not_verified", trusted: false },
      }),
    );
    expect(screen.getByText("No decision brief yet")).toBeInTheDocument();
    expect(within(fact("Pay")).getByText("Unknown")).toBeInTheDocument();
    expect(within(fact("Location")).getByText("Location not stated")).toBeInTheDocument();
    expect(within(fact("Eligibility")).getByText("Not checked")).toBeInTheDocument();
    expect(within(fact("Eligibility")).queryByRole("button")).not.toBeInTheDocument();
    expect(within(fact("Listing")).getByText("Not verified at the employer yet")).toBeInTheDocument();
    expect(screen.getByText("The listing has no description.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Full reasoning" })).not.toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("actions", () => {
  it("offers Today's four decisions, Save last and primary", () => {
    brief();
    const buttons = screen
      .getAllByRole("button")
      .map((b) => b.textContent ?? "")
      .filter((t) => ["Not now", "Not for me", "I applied", "Save"].includes(t));
    expect(buttons).toEqual(["Not now", "Not for me", "I applied", "Save"]);
  });

  it("records a decision and says what happened", async () => {
    const act = actions();
    render(<OpportunityBrief job={jobDetail()} context={null} actions={act} now={now} />);
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(act.feedback).toHaveBeenCalledWith("opp_0123456789abcdef0123456789abcdef", "save");
    expect(await screen.findByRole("status")).toHaveTextContent("Saved. It's in Applications.");
  });

  it("points to Applications once it is in the pipeline, with the history below", () => {
    brief(
      jobDetail({
        pipeline: {
          stage: "applied",
          furthest: "applied",
          since: "2026-09-24T10:00:00Z",
          feedback: [{ id: "fb_1", action: "applied", at: "2026-09-24T10:00:00Z" }],
        },
      }),
    );
    expect(screen.queryByRole("button", { name: "Save" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Manage it in Applications" })).toBeInTheDocument();
    expect(screen.getByRole("region", { name: "Your history" })).toHaveTextContent(/Applied since 1 day ago/);
  });
});
