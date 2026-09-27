import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PreferenceControls as Controls } from "@/lib/api-types";

import { controls, stated } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { ClarifyPreference } from "./clarify";
import { CompanyControls, LayerLegend, LocationControls, PayControls, WorkControls } from "./preference-controls";

function update() {
  return vi.fn(async () => ({ ok: true as const, data: { unchanged: false } as never }));
}

function group(name: string) {
  return screen.getByRole("group", { name });
}

describe("the three layers", () => {
  it("says what requirements, preferences and learned taste each do", async () => {
    const { container } = render(<LayerLegend />);
    const legend = screen.getByLabelText("How Narrow uses what you tell it");
    expect(within(legend).getByText("Requirement")).toBeInTheDocument();
    expect(within(legend).getByText(/never a Strong fit/)).toBeInTheDocument();
    expect(within(legend).getByText("Preference")).toBeInTheDocument();
    expect(within(legend).getByText(/Never leaves a job out/)).toBeInTheDocument();
    expect(within(legend).getByText("Learned")).toBeInTheDocument();
    expect(within(legend).getByText(/Ranking only/)).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("Work", () => {
  it("shows the work setup and its layer, and replaces it with one answer", async () => {
    const save = update();
    const { container } = render(<WorkControls controls={controls()} update={save} />);
    const setup = group("Work setup");
    expect(within(setup).getByRole("radio", { name: /Remote only/ })).toBeChecked();
    expect(within(setup).getByText("Requirement")).toBeInTheDocument();
    await userEvent.click(within(setup).getByRole("radio", { name: /Prefer remote/ }));
    expect(save).toHaveBeenCalledWith([{ kind: "work_setup", setup: "prefer_remote" }], []);
    expect(await within(setup).findByText("Saved.")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("keeps relocation a separate answer, with places when only some", async () => {
    const save = update();
    render(<WorkControls controls={controls()} update={save} />);
    const relocation = group("Relocation");
    expect(within(relocation).getByRole("radio", { name: /Not willing to relocate/ })).toBeChecked();
    await userEvent.click(within(relocation).getByRole("radio", { name: /Only to some places/ }));
    expect(save).not.toHaveBeenCalled();
    await userEvent.type(within(relocation).getByLabelText("Countries or regions"), "Portugal, Spain");
    await userEvent.click(within(relocation).getByRole("button", { name: "Save places" }));
    expect(save).toHaveBeenCalledWith([{ kind: "relocation", willing: true, only_to: ["Portugal", "Spain"] }], []);
    await userEvent.click(within(relocation).getByRole("radio", { name: /Open to relocation/ }));
    expect(save).toHaveBeenLastCalledWith([{ kind: "relocation", willing: true, only_to: [] }], []);
  });

  it("says when stored work modes are none of the five answers", () => {
    const c = controls();
    const custom: Controls = { ...c, work: { ...c.work, setup: "custom", custom: "wanted hybrid, unwanted on-site", setup_layer: "preference" } };
    render(<WorkControls controls={custom} update={update()} />);
    expect(screen.getByText(/From your words: wanted hybrid, unwanted on-site/)).toBeInTheDocument();
    expect(within(group("Work setup")).getAllByRole("radio").filter((r) => (r as HTMLInputElement).checked)).toHaveLength(0);
  });
});

describe("Location", () => {
  it("reads where the person lives into the remote scopes that include them, and never calls bare Remote global", () => {
    render(<LocationControls controls={controls()} update={update()} />);
    const home = group("Where you live");
    expect(within(home).getByLabelText("Where you live")).toHaveValue("Brazil");
    expect(within(home).getByText(/anywhere, the Americas, Latin America, South America or Brazil include you/)).toBeInTheDocument();
    expect(within(home).getByText(/only says “Remote” doesn't say where/)).toBeInTheDocument();
  });

  it("adds where the person may legally work, apart from where they live", async () => {
    const save = update();
    render(<LocationControls controls={controls()} update={save} />);
    const legal = group("Where you may legally work");
    expect(within(legal).getByText(/stays unresolved/)).toBeInTheDocument();
    await userEvent.type(within(legal).getByLabelText("Add a country or region"), "Portugal");
    await userEvent.click(within(legal).getByRole("button", { name: "Add" }));
    expect(save).toHaveBeenCalledWith([{ kind: "authorized_in", place: "Portugal" }], []);
  });

  it("toggles remote scopes with one importance for all of them", async () => {
    const save = update();
    const c = controls();
    const latam = {
      record: stated({ id: "pref_latam", value: "work in Latin America", stance: "wanted", layer: "preference" }),
      place: "Latin America",
      read_as: "Latin America",
      code: "latam",
    };
    const withScope: Controls = { ...c, location: { ...c.location, remote_geography: [latam] } };
    render(<LocationControls controls={withScope} update={save} />);
    const scopes = group("Remote roles open to");
    expect(within(scopes).getByRole("checkbox", { name: "Latin America (LATAM)" })).toBeChecked();
    expect(within(scopes).getByText("Preference")).toBeInTheDocument();
    await userEvent.click(within(scopes).getByRole("checkbox", { name: "Anywhere (no restriction)" }));
    expect(save).toHaveBeenCalledWith([{ kind: "region", region: "Worldwide", stance: "want" }], []);
    await userEvent.click(within(scopes).getByRole("checkbox", { name: "Latin America (LATAM)" }));
    expect(save).toHaveBeenLastCalledWith([], ["pref_latam"]);
    await userEvent.click(within(scopes).getByRole("radio", { name: "Must have" }));
    expect(save).toHaveBeenLastCalledWith([{ kind: "region", region: "Latin America", stance: "require" }], []);
  });

  it("sets the policy for unclear eligibility", async () => {
    const save = update();
    render(<LocationControls controls={controls()} update={save} />);
    const policy = group("When eligibility is unclear");
    expect(within(policy).getByRole("radio", { name: /Show me, marked unresolved/ })).toBeChecked();
    await userEvent.click(within(policy).getByRole("radio", { name: /Only when it's confirmed/ }));
    expect(save).toHaveBeenCalledWith([{ kind: "unclear_eligibility", show: false }], []);
  });
});

describe("Pay", () => {
  it("shows the minimum as a requirement and the target as a preference", () => {
    const c = controls();
    const withTarget: Controls = {
      ...c,
      pay: {
        ...c.pay,
        target: [{ record: stated({ id: "pref_target", stance: "wanted", layer: "preference" }), amount: 180000, currency: "USD", period: "year" }],
      },
    };
    render(<PayControls controls={withTarget} update={update()} />);
    const minimum = group("Minimum");
    expect(within(minimum).getByText("At least USD 140,000 per year")).toBeInTheDocument();
    expect(within(minimum).getByText("Requirement")).toBeInTheDocument();
    expect(within(minimum).getByText(/never counts as meeting it/)).toBeInTheDocument();
    const target = group("Target");
    expect(within(target).getByText("Around USD 180,000 per year")).toBeInTheDocument();
    expect(within(target).getByText("Preference")).toBeInTheDocument();
  });

  it("never assumes a currency", async () => {
    const save = update();
    const c = controls();
    render(<PayControls controls={{ ...c, pay: { ...c.pay, minimum: [] } }} update={save} />);
    const minimum = group("Minimum");
    expect(within(minimum).getByLabelText("Currency")).toHaveValue("");
    await userEvent.type(within(minimum).getByLabelText("Amount"), "140,000");
    await userEvent.click(within(minimum).getByRole("button", { name: "Set" }));
    expect(save).not.toHaveBeenCalled();
    expect(within(minimum).getByRole("alert")).toHaveTextContent("Narrow never assumes one");
    await userEvent.type(within(minimum).getByLabelText("Currency"), "usd");
    await userEvent.selectOptions(within(minimum).getByLabelText("Per"), "month");
    await userEvent.click(within(minimum).getByRole("button", { name: "Set" }));
    expect(save).toHaveBeenCalledWith(
      [{ kind: "compensation", minimum: 140000, target: null, currency: "USD", period: "month", applies_to: null }],
      [],
    );
  });

  it("sets the unknown-pay policy on its own", async () => {
    const save = update();
    const { container } = render(<PayControls controls={controls()} update={save} />);
    const policy = group("When pay isn't published");
    expect(within(policy).getByRole("radio", { name: /Show them, marked unresolved/ })).toBeChecked();
    await userEvent.click(within(policy).getByRole("radio", { name: /Hide them/ }));
    expect(save).toHaveBeenCalledWith([{ kind: "unknown_pay", show: false }], []);
    expect(await violations(container)).toEqual([]);
  });
});

describe("Company & team", () => {
  it("keeps team size, company size and stage apart, each must have or nice to have", async () => {
    const save = update();
    const { container } = render(<CompanyControls controls={controls()} update={save} />);
    expect(screen.getByRole("heading", { name: "Team" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Company size" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Stage" })).toBeInTheDocument();
    const team = screen.getByRole("group", { name: "Small team" });
    expect(within(team).getByRole("radio", { name: "Nice to have" })).toBeChecked();
    const company = screen.getByRole("group", { name: "Small company" });
    expect(within(company).getByRole("radio", { name: "Off" })).toBeChecked();
    await userEvent.click(within(team).getByRole("radio", { name: "Must have" }));
    expect(save).toHaveBeenCalledWith([{ kind: "company", company: "small_team", stance: "require" }], []);
    await userEvent.click(within(screen.getByRole("group", { name: "Early-stage" })).getByRole("radio", { name: "Nice to have" }));
    expect(save).toHaveBeenLastCalledWith([{ kind: "company", company: "early_stage", stance: "want" }], []);
    expect(await violations(container)).toEqual([]);
  });

  it("turns a kind off by removing it", async () => {
    const save = update();
    render(<CompanyControls controls={controls()} update={save} />);
    await userEvent.click(within(screen.getByRole("group", { name: "Small team" })).getByRole("radio", { name: "Off" }));
    expect(save).toHaveBeenCalledWith([], ["pref_team"]);
  });
});

describe("clarifying importance", () => {
  it("asks must have or nice to have, and answers with the same preference", async () => {
    const clarify = vi.fn(async () => ({ ok: true as const, data: {} as never }));
    const p = stated({
      id: "pref_remote",
      stance: "wanted",
      certainty: "uncertain",
      origin: "statement",
      layer: "preference",
      clarify: { kind: "importance", value: "remote work", input: { kind: "work_mode", mode: "remote", stance: "want" } },
    });
    const { container } = render(<ClarifyPreference p={p} clarify={clarify} />);
    expect(screen.getByText(/it's a nice-to-have/)).toBeInTheDocument();
    await userEvent.click(screen.getByLabelText("Must have"));
    await userEvent.click(screen.getByRole("button", { name: "Confirm" }));
    expect(clarify).toHaveBeenCalledWith("pref_remote", [{ kind: "work_mode", mode: "remote", stance: "require" }]);
    expect(await violations(container)).toEqual([]);
  });
});

describe("one source of truth", () => {
  it("resets a form when the stored value changes elsewhere (the person's words)", () => {
    const c = controls();
    const lisbon: Controls = { ...c, location: { ...c.location, home: "Lisbon, Portugal", home_country: "Portugal", home_basis: "resume" } };
    const { rerender } = render(<LocationControls controls={lisbon} update={update()} />);
    expect(within(group("Where you live")).getByRole("textbox")).toHaveValue("Lisbon, Portugal");
    expect(screen.getByText(/From your resume/)).toBeInTheDocument();
    rerender(<LocationControls controls={c} update={update()} />);
    expect(within(group("Where you live")).getByRole("textbox")).toHaveValue("Brazil");
  });

  it("resets the pay form to the stored minimum", () => {
    const c = controls();
    const { rerender } = render(<PayControls controls={c} update={update()} />);
    expect(within(group("Minimum")).getByLabelText("Amount")).toHaveValue("140000");
    const raised: Controls = {
      ...c,
      pay: { ...c.pay, minimum: [{ ...c.pay.minimum[0]!, amount: 150000, record: stated({ id: "pref_min_2", category: "compensation" }) }] },
    };
    rerender(<PayControls controls={raised} update={update()} />);
    expect(within(group("Minimum")).getByLabelText("Amount")).toHaveValue("150000");
  });
});

describe("pay periods (Codex review #3)", () => {
  for (const period of ["hour", "day", "month", "year"] as const) {
    it(`keeps a USD 100 per ${period} figure exactly when updated unchanged`, async () => {
      const save = update();
      const c = controls();
      const figure: Controls = {
        ...c,
        pay: {
          ...c.pay,
          minimum: [{ record: stated({ id: "pref_min", category: "compensation" }), amount: 100, currency: "USD", period }],
        },
      };
      render(<PayControls controls={figure} update={save} />);
      const minimum = group("Minimum");
      expect(within(minimum).getByText(`At least USD 100 per ${period}`)).toBeInTheDocument();
      expect(within(minimum).getByLabelText("Per")).toHaveValue(period);
      await userEvent.click(within(minimum).getByRole("button", { name: "Update" }));
      expect(save).toHaveBeenCalledWith(
        [{ kind: "compensation", minimum: 100, target: null, currency: "USD", period, applies_to: null }],
        [],
      );
    });
  }

  it("offers every period the API stores", () => {
    render(<PayControls controls={controls()} update={update()} />);
    const options = within(within(group("Target")).getByLabelText("Per")).getAllByRole("option");
    expect(options.map((o) => o.getAttribute("value"))).toEqual(["year", "month", "day", "hour"]);
  });
});

describe("Anywhere (production smoke test)", () => {
  it("is no restriction: no layer, and it says it ranks nothing", () => {
    const c = controls();
    const anywhere = {
      record: stated({ id: "pref_any", value: "remote roles open anywhere (no geographic restriction)", stance: "wanted", layer: "preference" }),
      place: "Worldwide",
      read_as: "anywhere",
      code: "worldwide",
    };
    render(<LocationControls controls={{ ...c, location: { ...c.location, remote_geography: [anywhere] } }} update={update()} />);
    const scopes = group("Remote roles open to");
    expect(within(scopes).getByRole("checkbox", { name: "Anywhere (no restriction)" })).toBeChecked();
    expect(within(scopes).queryByText("Preference", { exact: true })).not.toBeInTheDocument();
    expect(within(scopes).getByText(/doesn't rank remote roles up or down/)).toBeInTheDocument();
  });
});
