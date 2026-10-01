import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { TasteAction, TasteItemView, TasteProfileView, TasteUpdateResult } from "@/lib/api-types";

import { roles, tasteProfile } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { Constraints, DescribeForm, LearnedOverTime, LookingFor, TasteSummary } from "./taste-profile";

function answer(profile: TasteProfileView, action = "confirm"): { ok: true; data: TasteUpdateResult } {
  return { ok: true, data: { action, changed: true, interpreted: false, profile } };
}

function review(profile = tasteProfile()) {
  return vi.fn(async (action: TasteAction) => answer(profile, action.action));
}

describe("TasteSummary", () => {
  it("is a short summary of what they want and avoid, marked as Narrow's reading", async () => {
    const { container } = render(<TasteSummary profile={tasteProfile()} review={review()} />);
    const wants = screen.getByRole("list", { name: "What you want" });
    expect(within(wants).getAllByRole("listitem")).toHaveLength(3);
    expect(within(wants).getByText("Backend engineering · Platform engineering")).toBeInTheDocument();
    // Inferred, not said: it says where it comes from.
    expect(within(wants).getByText(/from your profile/)).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "You tend to avoid" })).toBeInTheDocument();
    expect(within(screen.getByRole("list", { name: "What you avoid" })).getByText("Early-career roles")).toBeInTheDocument();
    expect(screen.getByText("Narrow's reading · check it")).toBeInTheDocument();
    // No weights, scores or taxonomy in the default view.
    const shown = container.cloneNode(true) as HTMLElement;
    shown.querySelectorAll("details").forEach((d) => d.remove());
    expect(shown.textContent).not.toMatch(/confidence|work_shape|small_team|weight/);
    expect(await violations(container)).toEqual([]);
  });

  it("confirms the whole summary with Looks right", async () => {
    const confirmed = tasteProfile({ needs_confirmation: false, confirmed_at: "2026-09-29T12:01:00Z" });
    const act = review(confirmed);
    render(<TasteSummary profile={tasteProfile()} review={act} />);
    await userEvent.click(screen.getByRole("button", { name: "Looks right" }));
    expect(act).toHaveBeenCalledWith({ action: "confirm" });
    expect(await screen.findByText("You confirmed this")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Looks right" })).not.toBeInTheDocument();
  });

  it("corrects one line at a time in a focused sheet", async () => {
    const act = review();
    render(<TasteSummary profile={tasteProfile()} review={act} />);
    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    const sheet = screen.getByRole("dialog", { name: "What Narrow understands" });
    // Change: new words and how they feel about it.
    await userEvent.click(within(sheet).getByRole("button", { name: "Change Small technical teams" }));
    const words = within(sheet).getByLabelText("In your words");
    await userEvent.clear(words);
    await userEvent.type(words, "Small or mid-size teams");
    await userEvent.selectOptions(within(sheet).getByLabelText("How you feel about it"), "open");
    await userEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(act).toHaveBeenCalledWith({ action: "correct", id: "taste_team", text: "Small or mid-size teams", polarity: "open" });
    // Doesn't matter, and remove.
    await userEvent.click(within(sheet).getByRole("button", { name: "Doesn't matter (Senior roles)" }));
    expect(act).toHaveBeenCalledWith({ action: "neutral", id: "taste_senior" });
    await userEvent.click(within(sheet).getByRole("button", { name: "Remove Early-career roles" }));
    expect(act).toHaveBeenCalledWith({ action: "remove", id: "taste_early" });
    // One more sentence.
    await userEvent.type(within(sheet).getByLabelText("Add one sentence"), "I'd love developer tooling");
    await userEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    expect(act).toHaveBeenCalledWith({ action: "add", text: "I'd love developer tooling" });
    expect(await violations(sheet)).toEqual([]);
  });

  it("says when an added sentence names a kind of role", async () => {
    const chose = vi.fn(async (action: TasteAction) => answer(tasteProfile({ roles: roles(["developer_tooling"]) }), action.action));
    render(<TasteSummary profile={tasteProfile()} review={chose} />);
    await userEvent.click(screen.getByRole("button", { name: "Edit" }));
    const sheet = screen.getByRole("dialog", { name: "What Narrow understands" });
    await userEvent.type(within(sheet).getByLabelText("Add one sentence"), "I'd love developer tooling");
    await userEvent.click(within(sheet).getByRole("button", { name: "Add" }));
    expect(await within(sheet).findByText("Added Developer tooling to the kinds of role you're looking for.")).toBeInTheDocument();
    expect(within(sheet).getByLabelText("Add one sentence")).toHaveValue("");
  });

  it("shows a reading set aside against a chosen role, to settle", () => {
    const aside = { ...tasteProfile().avoid[0]!.items[0]!, id: "taste_ai", dimension: "domain", value: "ai", text: "Ai" };
    render(<TasteSummary profile={tasteProfile({ roles: roles(["ml_product"]), set_aside: [aside] })} review={review()} />);
    expect(screen.getByText(/Narrow read “Ai” as something you'd avoid, which goes against the kind of role you chose/)).toBeInTheDocument();
  });

  it("keeps provenance one tap away", async () => {
    render(<TasteSummary profile={tasteProfile()} review={review()} />);
    await userEvent.click(screen.getByText("How Narrow read this"));
    expect(screen.getAllByText("Narrow's reading of your words").length).toBeGreaterThan(0);
    expect(screen.getByText(/latest title “Senior Software Engineer”/)).toBeInTheDocument();
    expect(screen.getByText(/medium confidence/)).toBeInTheDocument();
  });

  it("says when there is nothing yet", () => {
    render(<TasteSummary profile={tasteProfile({ understood: [], avoid: [], looking_for: null, looking_for_source: null, needs_confirmation: false })} review={review()} />);
    expect(screen.getByText(/Tell Narrow what you're looking for/)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Edit" })).not.toBeInTheDocument();
  });
});

describe("Anything else you care about", () => {
  it("asks the one question when nothing was said", async () => {
    const describe_ = vi.fn(async (_s: unknown, form: FormData) => ({ result: answer(tasteProfile()).data, submitted: String(form.get("text")) }));
    const onDone = vi.fn();
    const { container } = render(<DescribeForm describe={describe_} onDone={onDone} submitLabel="Continue" />);
    await userEvent.type(screen.getByRole("textbox", { name: "Anything else you care about?" }), "Small teams, backend work");
    await userEvent.click(screen.getByRole("button", { name: "Continue" }));
    await waitFor(() => expect(onDone).toHaveBeenCalled());
    expect(describe_).toHaveBeenCalledOnce();
    expect(await violations(container)).toEqual([]);
  });

  it("shows an error and keeps the words", async () => {
    const describe_ = vi.fn(async () => ({ error: { title: "That didn't work", message: "say it in a few words" }, submitted: "x" }));
    render(<DescribeForm describe={describe_} />);
    await userEvent.type(screen.getByRole("textbox", { name: "Anything else you care about?" }), "x");
    await userEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("say it in a few words");
  });

  it("shows the words, edited in place", async () => {
    render(<LookingFor profile={tasteProfile()} describe={vi.fn()} review={review()} />);
    expect(screen.getByText(/I like small technical teams/)).toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Edit what else you care about" }));
    expect(screen.getByRole("textbox", { name: "Anything else you care about?" })).toHaveValue(tasteProfile().looking_for);
  });

  it("offers earlier words as the starting point", async () => {
    const act = review();
    render(<LookingFor profile={tasteProfile({ looking_for_source: "statements", looking_for: "remote only, small teams" })} describe={vi.fn()} review={act} />);
    expect(screen.getByText("What you told Narrow before:")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Use these words" }));
    expect(act).toHaveBeenCalledWith({ action: "reinterpret" });
  });
});

describe("Constraints and learned taste", () => {
  it("lists practical constraints compactly, with the settings behind Edit", async () => {
    render(
      <Constraints items={tasteProfile().constraints} noted={[]}>
        <p>the structured settings</p>
      </Constraints>,
    );
    const list = screen.getByRole("list", { name: "Your practical constraints" });
    expect(within(list).getByText("Remote only")).toBeInTheDocument();
    expect(screen.getByText("the structured settings")).not.toBeVisible();
    await userEvent.click(screen.getByText("Edit constraints"));
    expect(screen.getByText("the structured settings")).toBeVisible();
  });

  it("mentions practical words it didn't set", () => {
    render(<Constraints items={[]} noted={["remote"]} />);
    expect(screen.getByText(/You mentioned remote/)).toBeInTheDocument();
  });

  it("learned taste is its own compact section, never said by the person", () => {
    const { container } = render(<LearnedOverTime items={[]} />);
    expect(container).toBeEmptyDOMElement();
    const learned: TasteItemView = { ...tasteProfile().understood[2]!.items[0]!, id: "taste_l", origin: "learned", basis: "Learned from your feedback", sources: [{ kind: "feedback", text: "Your feedback: 3 saves" }] };
    render(<LearnedOverTime items={[learned]} />);
    expect(screen.getByText(/You tend to go for small technical teams/)).toBeInTheDocument();
    expect(screen.getByText(/3 saves/)).toBeInTheDocument();
    expect(screen.getByText(/only changes the order/)).toBeInTheDocument();
  });
});
