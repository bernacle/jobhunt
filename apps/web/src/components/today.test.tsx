import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { feedView } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { CaughtUp } from "./caught-up";
import { DiscoveryStatus, FeedSummary } from "./feed-summary";
import { RefreshButton } from "./refresh-on-focus";

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

describe("DiscoveryStatus", () => {
  it("says when job boards were last read and will be next", () => {
    render(<DiscoveryStatus discovery={feedView().discovery} now={now} />);
    expect(screen.getByText("Checked 20 min ago · next check in about 30 min")).toBeInTheDocument();
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

  it("before the first discovery, says Narrow is still gathering", () => {
    const feed = feedView({
      items: [],
      caught_up: true,
      summary: { checked: 0, passed_eligibility: 0, worth_reviewing: 0, new: 0, changed: 0, shown: 0 },
      pipeline: { saved: 0, applied: 0, interviewing: 0, offer: 0 },
      discovery: { mode: "background" },
    });
    render(<CaughtUp feed={feed} now={now} />);
    expect(screen.getByRole("heading", { name: "Narrow is still gathering jobs." })).toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
  });
});

describe("CaughtUp, told apart", () => {
  it("says nothing was worth showing when no open job fit, not that the person is caught up", () => {
    const feed = feedView({
      items: [],
      caught_up: true,
      passed_over: 0,
      summary: { checked: 2882, passed_eligibility: 2600, worth_reviewing: 0, new: 0, changed: 0, shown: 0 },
      pipeline: { saved: 0, applied: 0, interviewing: 0, offer: 0 },
    });
    render(<CaughtUp feed={feed} now={now} />);
    expect(screen.getByRole("heading", { name: "Nothing worth your time yet." })).toBeInTheDocument();
    expect(screen.getByText(/checked 2,882 open jobs and none fits well enough to show/)).toBeInTheDocument();
    expect(screen.queryByText("You're caught up.")).not.toBeInTheDocument();
  });

  it("says caught up once the person has been through what was worth it", () => {
    const feed = feedView({
      items: [],
      caught_up: true,
      passed_over: 3,
      summary: { checked: 2882, passed_eligibility: 2600, worth_reviewing: 3, new: 0, changed: 0, shown: 0 },
      pipeline: { saved: 0, applied: 0, interviewing: 0, offer: 0 },
    });
    render(<CaughtUp feed={feed} now={now} />);
    expect(screen.getByRole("heading", { name: "You're caught up." })).toBeInTheDocument();
  });
});

describe("RefreshButton", () => {
  it("while checking, says so quietly, with no progress it doesn't have", () => {
    const { rerender } = render(
      <>
        <ol aria-label="Recommendations">
          <li>Senior Backend Engineer (Go)</li>
        </ol>
        <RefreshButton pending={false} onRefresh={() => {}} />
      </>,
    );
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    rerender(
      <>
        <ol aria-label="Recommendations">
          <li>Senior Backend Engineer (Go)</li>
        </ol>
        <RefreshButton pending onRefresh={() => {}} />
      </>,
    );
    const status = screen.getByRole("status");
    expect(status).toHaveTextContent("Checking for new opportunities…");
    expect(status.textContent).not.toMatch(/\d|%/);
    // The list already on screen stays while it checks.
    expect(screen.getByRole("list", { name: "Recommendations" })).toHaveTextContent("Senior Backend Engineer (Go)");
  });

  it("checks again on demand", async () => {
    const onRefresh = vi.fn();
    render(<RefreshButton pending={false} onRefresh={onRefresh} />);
    await userEvent.click(screen.getByRole("button", { name: "Check again" }));
    expect(onRefresh).toHaveBeenCalledOnce();
  });
});
