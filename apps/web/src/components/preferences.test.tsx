import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PreferenceUpdateResult } from "@/lib/api-types";

import { tasteView } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { LearnedTaste } from "./learned-taste";
import { Interpretation, PreciseForm, StatementForm } from "./preferences";

const result: PreferenceUpdateResult = {
  statement: { id: "stmt_1", text: "…", reading: "partial", not_understood: ["something about vibes"], at: "2026-09-25T12:00:00Z" },
  interpreted: [
    { id: "pref_1", category: "company", stance: "wanted", value: "small teams", certainty: "certain", origin: "statement", active: true },
    { id: "pref_2", category: "compensation", stance: "required", value: "at least USD 140,000 per year", certainty: "certain", origin: "statement", active: true },
    { id: "pref_3", category: "role", stance: "unwanted", value: "SRE roles", certainty: "uncertain", origin: "statement", note: "read 'pure SRE' as SRE roles", active: true },
  ],
  uncertain: [
    { id: "pref_3", category: "role", stance: "unwanted", value: "SRE roles", certainty: "uncertain", origin: "statement", note: "read 'pure SRE' as SRE roles", active: true },
  ],
  not_understood: ["something about vibes"],
  replaced: [],
  removed: [],
  unchanged: false,
  active: [],
};

describe("preference interpretation", () => {
  it("shows what was understood, what is uncertain, and what wasn't interpreted", () => {
    render(<Interpretation result={result} />);
    const understood = screen.getByRole("heading", { name: "Understood" }).nextElementSibling as HTMLElement;
    expect(within(understood).getByText(/small teams/)).toBeInTheDocument();
    expect(within(understood).getByText(/at least USD 140,000 per year/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Not sure I read these right" })).toBeInTheDocument();
    expect(screen.getByText(/read 'pure SRE' as SRE roles/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "I couldn't interpret" })).toBeInTheDocument();
    expect(screen.getByText("something about vibes")).toBeInTheDocument();
  });

  it("sends the person's words and shows the answer", async () => {
    const action = vi.fn(async (_state: unknown, form: FormData) => ({
      result,
      submitted: String(form.get("statement")),
    }));
    const { container } = render(<StatementForm action={action} />);
    await userEvent.type(
      screen.getByLabelText("What are you looking for?"),
      "I want small product teams, at least $140k USD, and no pure SRE roles.",
    );
    await userEvent.click(screen.getByRole("button", { name: "Update preferences" }));
    expect(await screen.findByText("something about vibes")).toBeInTheDocument();
    expect(action).toHaveBeenCalledOnce();
    expect(await violations(container)).toEqual([]);
  });

  it("sets pay precisely", async () => {
    const set = vi.fn(async () => ({ ok: true as const, data: { ...result, unchanged: false } }));
    render(<PreciseForm set={set} />);
    await userEvent.type(screen.getByLabelText(/Minimum/), "140,000");
    await userEvent.click(screen.getByRole("button", { name: "Save pay" }));
    expect(set).toHaveBeenCalledWith({ kind: "compensation", minimum: 140000, target: null, currency: "USD", period: "year" });
    expect(await screen.findByText("Saved.")).toBeInTheDocument();
  });
});

describe("learned taste", () => {
  it("is labeled as learned, with its evidence, apart from what was stated", async () => {
    const { container } = render(<LearnedTaste taste={tasteView()} />);
    expect(screen.getByText("Learned, not stated")).toBeInTheDocument();
    expect(screen.getByText(/What you tell JobHunt always wins/)).toBeInTheDocument();
    expect(screen.getByText("You tend to pass on SRE / DevOps")).toBeInTheDocument();
    expect(screen.getByText(/established · 2 reasons in your words across 2 jobs/)).toBeInTheDocument();
    await userEvent.click(screen.getByText("Why JobHunt thinks so"));
    expect(screen.getByText("too much SRE")).toBeInTheDocument();
    // Stated preferences are not rendered as learned.
    expect(screen.queryByText(/backend roles/)).not.toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("says when nothing has been learned yet", () => {
    render(<LearnedTaste taste={tasteView({ learned: [], feedback_events: 0, opportunities: 0 })} />);
    expect(screen.getByText(/Nothing yet\./)).toBeInTheDocument();
  });

  it("keeps contradictory patterns apart and unused", () => {
    render(<LearnedTaste taste={tasteView({ learned: [], contradictory: [{ ...tasteView().learned[0]!, key: "domain:fintech", status: "mixed", confidence: null, value: "fintech", dimension: "domain" }] })} />);
    expect(screen.getByText("Contradictory, so not used (1)")).toBeInTheDocument();
  });
});
