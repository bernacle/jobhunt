import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PreferenceUpdateResult } from "@/lib/api-types";

import { learned, tasteView } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { AddPreference, AddPreferenceRow, Interpretation, StatementForm } from "./preferences";
import { NotInUse, TasteReview } from "./taste";

const result: PreferenceUpdateResult = {
  statement: { id: "stmt_1", text: "…", reading: "partial", not_understood: ["something about vibes"], at: "2026-09-25T12:00:00Z" },
  interpreted: [
    { id: "pref_1", category: "company", stance: "wanted", value: "small teams", certainty: "certain", origin: "statement", active: true, layer: "preference" },
    { id: "pref_2", category: "compensation", stance: "required", value: "at least USD 140,000 per year", certainty: "certain", origin: "statement", active: true, layer: "preference" },
    { id: "pref_3", category: "role", stance: "unwanted", value: "SRE roles", certainty: "uncertain", origin: "statement", note: "read 'pure SRE' as SRE roles", active: true, layer: "preference" },
  ],
  uncertain: [
    { id: "pref_3", category: "role", stance: "unwanted", value: "SRE roles", certainty: "uncertain", origin: "statement", note: "read 'pure SRE' as SRE roles", active: true, layer: "preference" },
  ],
  not_understood: ["something about vibes"],
  replaced: [],
  removed: [],
  unchanged: false,
  active: [],
};

const ok = { ok: true as const, data: { ...result, unchanged: false } };

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
    const action = vi.fn(async (_state: unknown, form: FormData) => ({ result, submitted: String(form.get("statement")) }));
    const { container } = render(<StatementForm action={action} />);
    await userEvent.type(screen.getByLabelText("What are you looking for?"), "I want small product teams, at least $140k USD, and no pure SRE roles.");
    await userEvent.click(screen.getByRole("button", { name: "Update preferences" }));
    expect(await screen.findByText("something about vibes")).toBeInTheDocument();
    expect(action).toHaveBeenCalledOnce();
    expect(await violations(container)).toEqual([]);
  });
});

describe("AddPreference", () => {
  it("adds a structured preference: about, rule, value", async () => {
    const set = vi.fn(async () => ok);
    const { container } = render(<AddPreference set={set} />);
    await userEvent.selectOptions(screen.getByLabelText("About"), "domain");
    await userEvent.selectOptions(screen.getByLabelText("Rule"), "avoid");
    await userEvent.type(screen.getByLabelText("Value"), "adtech");
    await userEvent.click(screen.getByRole("button", { name: "Add preference" }));
    expect(set).toHaveBeenCalledWith({ kind: "domain", domain: "adtech", stance: "avoid" });
    expect(await screen.findByText("Saved.")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("leaves work setup, location, pay and company size to the structured settings", async () => {
    render(<AddPreference set={vi.fn(async () => ok)} />);
    const about = screen.getByLabelText("About");
    const kinds = within(about).getAllByRole("option").map((o) => o.getAttribute("value"));
    expect(kinds).toEqual(["role", "domain", "work_style", "company", "timezone"]);
    // No currency is ever filled in for the person.
    expect(screen.queryByLabelText("Currency")).not.toBeInTheDocument();
  });
});

describe("AddPreferenceRow", () => {
  it("adds one preference in a focused sheet, saying what a rule does, and returns to the row", async () => {
    const set = vi.fn(async () => ok);
    const { container } = render(<AddPreferenceRow set={set} />);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Add a preference" }));
    const sheet = screen.getByRole("dialog", { name: "Add a preference" });
    expect(within(sheet).getByLabelText("About")).toHaveFocus();
    expect(within(sheet).getByText("Changes the order. Never leaves a job out.")).toBeInTheDocument();
    await userEvent.selectOptions(within(sheet).getByLabelText("Rule"), "require");
    // A requirement says what it does, including when a posting doesn't say.
    expect(within(sheet).getByText(/is left out\. If the posting doesn't say, it stays unresolved/)).toBeInTheDocument();
    await userEvent.click(within(sheet).getByRole("button", { name: "Add preference" }));
    expect(within(sheet).getByRole("alert")).toHaveTextContent("Enter a value");
    expect(set).not.toHaveBeenCalled();
    expect(await violations(container)).toEqual([]);
    await userEvent.type(within(sheet).getByLabelText("Value"), "platform");
    await userEvent.click(within(sheet).getByRole("button", { name: "Add preference" }));
    expect(set).toHaveBeenCalledWith({ kind: "role", role: "platform", stance: "require" });
    expect(await screen.findByText("Saved.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("Cancel adds nothing", async () => {
    const set = vi.fn(async () => ok);
    render(<AddPreferenceRow set={set} />);
    await userEvent.click(screen.getByRole("button", { name: "Add a preference" }));
    await userEvent.type(screen.getByLabelText("Value"), "adtech");
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(set).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add a preference" })).toBeInTheDocument();
  });
});

describe("explicit and learned", () => {
  const remove = vi.fn(async () => ok);

  it("keeps what was told apart from what was learned, each in its own layer", async () => {
    const { container } = render(<TasteReview taste={tasteView()} remove={remove} />);
    const preferences = screen.getByRole("region", { name: "Preferences" });
    expect(within(preferences).getByText("Want: backend roles")).toBeInTheDocument();
    expect(within(preferences).queryByText(/SRE/)).not.toBeInTheDocument();
    expect(within(screen.getByRole("region", { name: "Requirements" })).getByText("None stated")).toBeInTheDocument();
    const learnedLayer = screen.getByRole("region", { name: "Learned" });
    expect(within(learnedLayer).getByText("You tend to pass on SRE / DevOps")).toBeInTheDocument();
    expect(within(learnedLayer).getByText(/established · 2 reasons in your words across 2 jobs/)).toBeInTheDocument();
    await userEvent.click(within(learnedLayer).getByText("Why Narrow thinks so"));
    expect(screen.getByText("too much SRE")).toBeInTheDocument();
    // A learned tendency has no requirement controls.
    expect(within(learnedLayer).queryByRole("button", { name: /Remove/ })).not.toBeInTheDocument();
    expect(within(preferences).getByRole("button", { name: /Remove/ })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("says when nothing has been learned yet", () => {
    render(<TasteReview taste={tasteView({ learned: [], feedback_events: 0, opportunities: 0 })} remove={remove} />);
    expect(screen.getByText("Nothing learned yet")).toBeInTheDocument();
  });

  it("keeps contradictory patterns apart and unused", () => {
    render(<NotInUse taste={tasteView({ learned: [], contradictory: [learned({ key: "domain:fintech", status: "mixed", confidence: null, value: "fintech", dimension: "domain" })] })} />);
    expect(screen.getByText("Contradictory, so not used (1)")).toBeInTheDocument();
  });
});
