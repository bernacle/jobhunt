import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import { feedView } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { CaughtUp } from "./caught-up";
import { FeedSummary } from "./feed-summary";

const now = new Date("2026-09-25T12:00:00Z");

describe("FeedSummary", () => {
  it("uses only the counts the API computed", () => {
    render(<FeedSummary summary={feedView().summary} />);
    expect(screen.getByText(/Checked 7,500 open jobs · 312 you could take · 18 looked promising/)).toBeInTheDocument();
    expect(screen.getByText("3 are worth your attention today")).toBeInTheDocument();
  });

  it("says nothing before anything was checked", () => {
    const { container } = render(
      <FeedSummary summary={{ checked: 0, passed_eligibility: 0, worth_reviewing: 0, new: 0, changed: 0, shown: 0 }} />,
    );
    expect(container).toBeEmptyDOMElement();
  });
});

describe("CaughtUp", () => {
  it("says the person is caught up, never '0 jobs found'", async () => {
    const feed = feedView({ items: [], caught_up: true });
    const { container } = render(<CaughtUp feed={feed} now={now} />);
    expect(screen.getByRole("heading", { name: "You're caught up." })).toBeInTheDocument();
    expect(screen.getByText(/keeps checking job boards in the background/)).toBeInTheDocument();
    expect(screen.getByText(/Job boards last read 20 min ago · next check in about 30 min/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "2 saved · 1 applied" })).toHaveAttribute("href", "/applications");
    expect(container.textContent).not.toMatch(/0 jobs|no jobs found/i);
    expect(await violations(container)).toEqual([]);
  });

  it("before the first discovery, says JobHunt is still gathering", () => {
    const feed = feedView({
      items: [],
      caught_up: true,
      summary: { checked: 0, passed_eligibility: 0, worth_reviewing: 0, new: 0, changed: 0, shown: 0 },
      pipeline: { saved: 0, applied: 0, interviewing: 0, offer: 0 },
      discovery: { mode: "background" },
    });
    render(<CaughtUp feed={feed} now={now} />);
    expect(screen.getByRole("heading", { name: "JobHunt is still gathering jobs." })).toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });
});
