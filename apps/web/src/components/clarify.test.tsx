import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PreferenceView } from "@/lib/api-types";

import { violations } from "../../test/axe";
import { ClarifyPreference } from "./clarify";

/** What onboarding read from "Small teams and the sallary of 140k". */
function read(overrides: Partial<PreferenceView>): PreferenceView {
  return {
    id: "pref_1",
    category: "compensation",
    stance: "wanted",
    value: "target 140,000 per year (currency unknown)",
    certainty: "uncertain",
    origin: "statement",
    snippet: "Small teams and the sallary of 140k",
    active: true,
    ...overrides,
  };
}

const pay = read({ clarify: { kind: "pay", amount: 140000, period: "year", bound: "target" } });
const size = read({ id: "pref_2", category: "company", value: "small teams", clarify: { kind: "size", value: "small_team" } });

function action() {
  return vi.fn(async () => ({ ok: true as const, data: {} as never }));
}

describe("ClarifyPreference", () => {
  it("keeps a pay without a currency visibly unresolved until confirmed", async () => {
    const { container } = render(<ClarifyPreference p={pay} clarify={action()} />);
    expect(screen.getByText("Needs your answer")).toBeInTheDocument();
    expect(screen.getByText(/isn't compared with any job's pay/)).toBeInTheDocument();
    expect(screen.getByLabelText("Currency")).toHaveValue("");
    expect(await violations(container)).toEqual([]);
  });

  it("turns the answer into an explicit floor in the chosen currency, replacing the reading", async () => {
    const clarify = action();
    render(<ClarifyPreference p={pay} clarify={clarify} />);
    await userEvent.click(screen.getByLabelText("At least (a hard floor)"));
    await userEvent.type(screen.getByLabelText("Currency"), "usd");
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(clarify).toHaveBeenCalledWith("pref_1", [
      { kind: "compensation", minimum: 140000, target: null, currency: "USD", period: "year", applies_to: null },
    ]);
  });

  it("never assumes a currency", async () => {
    const clarify = action();
    render(<ClarifyPreference p={pay} clarify={clarify} />);
    await userEvent.click(screen.getByLabelText("Around (what I'm aiming for)"));
    await userEvent.type(screen.getByLabelText("Currency"), "$");
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(clarify).not.toHaveBeenCalled();
    expect(screen.getByRole("alert")).toHaveTextContent("three-letter code");
  });

  it("asks whether small teams means the team or the company, and how strongly", async () => {
    const clarify = action();
    render(<ClarifyPreference p={size} clarify={clarify} />);
    expect(screen.getByText("For now it's a nice-to-have: it changes the order, and leaves nothing out.")).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Both"));
    await userEvent.click(screen.getByLabelText("Must have"));
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(clarify).toHaveBeenCalledWith("pref_2", [
      { kind: "company", company: "small_team", stance: "require" },
      { kind: "company", company: "small_company", stance: "require" },
    ]);
  });

  it("says what a must already does, and never that it leaves nothing out", () => {
    render(<ClarifyPreference p={{ ...size, stance: "required" }} clarify={action()} />);
    expect(screen.getByText(/For now it's a must: a posting that says otherwise is left out/)).toBeInTheDocument();
    expect(screen.queryByText(/leaves nothing out/)).not.toBeInTheDocument();
  });

  it("says a floor with a known currency leaves out pay below it, and a target doesn't", () => {
    const floor = read({ stance: "required", clarify: { kind: "pay", amount: 140000, period: "year", bound: "minimum", currency: "USD" } });
    const { unmount } = render(<ClarifyPreference p={floor} clarify={action()} />);
    expect(screen.getByText("For now it's a floor of USD 140,000 per year: verified pay below it is left out.")).toBeInTheDocument();
    unmount();
    const target = read({ clarify: { kind: "pay", amount: 140000, period: "year", bound: "target", currency: "USD" } });
    render(<ClarifyPreference p={target} clarify={action()} />);
    expect(screen.getByText(/target of USD 140,000 per year: it changes the order, and leaves nothing out/)).toBeInTheDocument();
  });
});
