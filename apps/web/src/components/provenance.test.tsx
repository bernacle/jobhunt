import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { SourceRecordView, VerificationBrief } from "@/lib/api-types";

import { violations } from "../../test/axe";
import { Provenance } from "./provenance";
import { VerificationStamp } from "./trust";

const now = new Date("2026-09-25T12:00:00Z");

function record(overrides: Partial<SourceRecordView> = {}): SourceRecordView {
  return {
    job_id: "job_1",
    source: "greenhouse:ledgerly",
    authority: "employer_configured_ats",
    status: "open",
    url: "https://boards.greenhouse.io/ledgerly/jobs/1",
    verification: "verified_active",
    first_seen_at: "2026-09-20T12:00:00Z",
    last_seen_at: "2026-09-25T11:00:00Z",
    last_success_at: "2026-09-25T11:42:00Z",
    ...overrides,
  };
}

function brief(overrides: Partial<VerificationBrief> = {}): VerificationBrief {
  return {
    state: "verified_active",
    trusted: true,
    freshness: "fresh",
    source: "greenhouse:ledgerly",
    authority: "employer_configured_ats",
    verified_at: "2026-09-25T11:42:00Z",
    ...overrides,
  };
}

/** The glyph of each provenance line: "fresh", "aging" or "none". */
function marks(): string[] {
  return screen.getAllByRole("listitem").map((li) => {
    const check = li.querySelector(".text-verified, .text-verified-aging");
    if (!check) return "none";
    return check.classList.contains("text-verified") ? "fresh" : "aging";
  });
}

describe("Provenance", () => {
  it("gives the current, authoritative source the mint check", async () => {
    const { container } = render(<Provenance sources={[record()]} verification={brief()} now={now} />);
    expect(marks()).toEqual(["fresh"]);
    expect(screen.getByRole("link", { name: /Greenhouse · ledgerly/ })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("never gives a stale verification the fresh mint check", () => {
    // The record still reads verified_active; the API judged it stale.
    render(<Provenance sources={[record()]} verification={brief({ freshness: "stale" })} now={now} />);
    expect(marks()).toEqual(["none"]);
  });

  it("gives an aging verification the grey check, not the mint one", () => {
    render(<Provenance sources={[record()]} verification={brief({ freshness: "aging" })} now={now} />);
    expect(marks()).toEqual(["aging"]);
  });

  it("gives no check when the listing isn't trusted", () => {
    render(<Provenance sources={[record()]} verification={brief({ trusted: false, not_trusted_because: "last verified 4 days ago" })} now={now} />);
    expect(marks()).toEqual(["none"]);
  });

  it("only marks the record the verification rests on, never a secondary or closed one", () => {
    render(
      <Provenance
        sources={[
          record(),
          record({ job_id: "job_2", source: "remoteboard:all", authority: "secondary_source", url: "https://example.test/2" }),
          record({ job_id: "job_3", status: "closed", url: "https://example.test/3" }),
        ]}
        verification={brief()}
        now={now}
      />,
    );
    expect(marks()).toEqual(["fresh", "none", "none"]);
  });
});

describe("VerificationStamp", () => {
  it("uses the same judgement: fresh gets the mint check, stale none", () => {
    const { rerender, container } = render(<VerificationStamp verification={brief()} now={now} />);
    expect(container.querySelector(".text-verified")).not.toBeNull();
    rerender(<VerificationStamp verification={brief({ freshness: "stale" })} now={now} />);
    expect(container.querySelector(".text-verified, .text-verified-aging")).toBeNull();
  });
});
