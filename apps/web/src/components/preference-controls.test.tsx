import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { PreferenceControls as Controls } from "@/lib/api-types";

import { controls, stated } from "../../test/fixtures";
import { violations } from "../../test/axe";
import { ClarifyPreference } from "./clarify";
import { CompanyControls, LayerTerms, LocationControls, PayControls, PreferenceEditing, WorkControls } from "./preference-controls";

function update() {
  return vi.fn(async () => ({ ok: true as const, data: { unchanged: false } as never }));
}

function row(name: string) {
  return screen.getByRole("group", { name });
}

/** Opens a row's editor with its one action (Edit, Add or Change). */
async function edit(name: string) {
  await userEvent.click(within(row(name)).getByRole("button", { name: new RegExp(`^(Edit|Add|Change) ${name}$`, "i") }));
  return row(name);
}

describe("the three layers", () => {
  it("says once what requirements, preferences and learned taste each do, and what unknown means", async () => {
    const { container } = render(<LayerTerms />);
    const terms = screen.getByLabelText("How Narrow uses what you tell it");
    expect(within(terms).getByText(/^Requirement/)).toBeInTheDocument();
    expect(within(terms).getByText(/never a Strong fit/)).toBeInTheDocument();
    expect(within(terms).getByText(/Never leaves a job out/)).toBeInTheDocument();
    expect(within(terms).getByText(/Ranking only/)).toBeInTheDocument();
    expect(within(terms).getByText(/Unknown never counts as meeting a requirement/)).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});

describe("a summary first", () => {
  it("shows each setting as its value and one action, with no editor open", async () => {
    const { container } = render(
      <>
        <WorkControls controls={controls()} update={update()} />
        <PayControls controls={controls()} update={update()} />
      </>,
    );
    expect(within(row("Work setup")).getByText("Remote only")).toBeInTheDocument();
    expect(within(row("Relocation")).getByText("Not willing to relocate")).toBeInTheDocument();
    expect(within(row("Minimum")).getByText("At least USD 140,000 per year")).toBeInTheDocument();
    expect(within(row("Target")).getByText("Not set")).toBeInTheDocument();
    expect(within(row("Target")).getByRole("button", { name: "Add target" })).toBeInTheDocument();
    expect(within(row("When pay isn't published")).getByRole("button", { name: "Change when pay isn't published" })).toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("edits one row at a time, and Escape cancels", async () => {
    render(
      <PreferenceEditing>
        <WorkControls controls={controls()} update={update()} />
      </PreferenceEditing>,
    );
    await edit("Work setup");
    expect(within(row("Work setup")).getAllByRole("radio")).toHaveLength(5);
    await edit("Relocation");
    expect(within(row("Work setup")).queryByRole("radio")).not.toBeInTheDocument();
    expect(within(row("Relocation")).getAllByRole("radio")).toHaveLength(3);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    await waitFor(() => expect(within(row("Relocation")).getByRole("button", { name: "Edit relocation" })).toHaveFocus());
  });
});

describe("Work", () => {
  it("replaces the work setup with one answer, saying the consequence of the chosen one only", async () => {
    const save = update();
    const { container } = render(<WorkControls controls={controls()} update={save} />);
    const setup = await edit("Work setup");
    expect(within(setup).getByRole("radio", { name: /Remote only/ })).toBeChecked();
    expect(within(setup).getByText("Hybrid and on-site roles are left out.")).toBeInTheDocument();
    expect(within(setup).queryByText("Remote roles rank higher. Nothing is left out.")).not.toBeInTheDocument();
    await userEvent.click(within(setup).getByRole("radio", { name: /Prefer remote/ }));
    expect(within(setup).getByText("Remote roles rank higher. Nothing is left out.")).toBeInTheDocument();
    expect(save).not.toHaveBeenCalled();
    expect(await violations(container)).toEqual([]);
    await userEvent.click(within(setup).getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith([{ kind: "work_setup", setup: "prefer_remote" }], []);
    expect(await within(row("Work setup")).findByText("Saved.")).toBeInTheDocument();
  });

  it("keeps relocation a separate answer, with places when only some", async () => {
    const save = update();
    render(<WorkControls controls={controls()} update={save} />);
    const relocation = await edit("Relocation");
    expect(within(relocation).getByRole("radio", { name: /Not willing to relocate/ })).toBeChecked();
    await userEvent.click(within(relocation).getByRole("radio", { name: /Only to some places/ }));
    await userEvent.click(within(relocation).getByRole("button", { name: "Save" }));
    expect(save).not.toHaveBeenCalled();
    expect(within(relocation).getByRole("alert")).toHaveTextContent("Name at least one country or region.");
    await userEvent.type(within(relocation).getByLabelText("Countries or regions"), "Portugal, Spain");
    await userEvent.click(within(relocation).getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith([{ kind: "relocation", willing: true, only_to: ["Portugal", "Spain"] }], []);
  });

  it("says when stored work modes are none of the five answers", async () => {
    const c = controls();
    const custom: Controls = { ...c, work: { ...c.work, setup: "custom", custom: "wanted hybrid, unwanted on-site", setup_layer: "preference" } };
    render(<WorkControls controls={custom} update={update()} />);
    expect(within(row("Work setup")).getByText(/From your words: wanted hybrid, unwanted on-site/)).toBeInTheDocument();
    const setup = await edit("Work setup");
    expect(within(setup).getAllByRole("radio").filter((r) => (r as HTMLInputElement).checked)).toHaveLength(0);
  });
});

describe("Location", () => {
  it("reads where the person lives into the remote scopes that include them, and never calls bare Remote global", async () => {
    render(<LocationControls controls={controls()} update={update()} />);
    expect(within(row("Where you live")).getByText("Brazil")).toBeInTheDocument();
    const home = await edit("Where you live");
    expect(within(home).getByLabelText("Where you live")).toHaveValue("Brazil");
    expect(within(home).getByText(/anywhere, the Americas, Latin America, South America or Brazil include you/)).toBeInTheDocument();
    expect(within(home).getByText(/only says “Remote” doesn't say where/)).toBeInTheDocument();
  });

  it("adds where the person may legally work, apart from where they live", async () => {
    const save = update();
    render(<LocationControls controls={controls()} update={save} />);
    expect(within(row("Authorized to work in")).getByText("Not set")).toBeInTheDocument();
    const legal = await edit("Authorized to work in");
    expect(within(legal).getByText(/stays unresolved/)).toBeInTheDocument();
    await userEvent.type(within(legal).getByLabelText("Add a country or region"), "Portugal");
    await userEvent.click(within(legal).getByRole("button", { name: "Add" }));
    expect(save).toHaveBeenCalledWith([{ kind: "authorized_in", place: "Portugal" }], []);
    await userEvent.click(within(legal).getByRole("button", { name: "Done" }));
    expect(within(row("Authorized to work in")).queryByRole("textbox")).not.toBeInTheDocument();
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
    expect(within(row("Remote roles open to")).getByText("Latin America")).toBeInTheDocument();
    expect(within(row("Remote roles open to")).getByText("Nice to have")).toBeInTheDocument();
    const scopes = await edit("Remote roles open to");
    expect(within(scopes).getByRole("checkbox", { name: "Latin America (LATAM)" })).toBeChecked();
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
    expect(within(row("When eligibility is unclear")).getByText("Show them, marked unresolved")).toBeInTheDocument();
    const policy = await edit("When eligibility is unclear");
    expect(within(policy).getByRole("radio", { name: /Show them, marked unresolved/ })).toBeChecked();
    await userEvent.click(within(policy).getByRole("radio", { name: /Only when it's confirmed/ }));
    await userEvent.click(within(policy).getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith([{ kind: "unclear_eligibility", show: false }], []);
  });
});

describe("Pay", () => {
  it("shows the minimum and the target, each with what it does when edited", async () => {
    const c = controls();
    const withTarget: Controls = {
      ...c,
      pay: {
        ...c.pay,
        target: [{ record: stated({ id: "pref_target", stance: "wanted", layer: "preference" }), amount: 180000, currency: "USD", period: "year" }],
      },
    };
    render(
      <PreferenceEditing>
        <PayControls controls={withTarget} update={update()} />
      </PreferenceEditing>,
    );
    expect(within(row("Minimum")).getByText("At least USD 140,000 per year")).toBeInTheDocument();
    expect(within(row("Target")).getByText("Around USD 180,000 per year")).toBeInTheDocument();
    expect(within(await edit("Minimum")).getByText(/never counts as meeting it/)).toBeInTheDocument();
    expect(within(await edit("Target")).getByText("Changes the order. Leaves nothing out.")).toBeInTheDocument();
  });

  it("never assumes a currency", async () => {
    const save = update();
    const c = controls();
    render(<PayControls controls={{ ...c, pay: { ...c.pay, minimum: [] } }} update={save} />);
    const minimum = await edit("Minimum");
    expect(within(minimum).getByLabelText("Currency")).toHaveValue("");
    await userEvent.type(within(minimum).getByLabelText("Amount"), "140,000");
    await userEvent.click(within(minimum).getByRole("button", { name: "Save" }));
    expect(save).not.toHaveBeenCalled();
    expect(within(minimum).getByRole("alert")).toHaveTextContent("Narrow never assumes one");
    await userEvent.type(within(minimum).getByLabelText("Currency"), "usd");
    await userEvent.selectOptions(within(minimum).getByLabelText("Per"), "month");
    await userEvent.click(within(minimum).getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith(
      [{ kind: "compensation", minimum: 140000, target: null, currency: "USD", period: "month", applies_to: null }],
      [],
    );
  });

  it("keeps an error beside the editor, and the old value in place", async () => {
    const save = vi.fn(async () => ({ ok: false as const, code: "conflict", title: "Something changed at the same time", message: "Nothing was lost. Try that again." }));
    render(<PayControls controls={controls()} update={save} />);
    const minimum = await edit("Minimum");
    await userEvent.clear(within(minimum).getByLabelText("Amount"));
    await userEvent.type(within(minimum).getByLabelText("Amount"), "150000");
    await userEvent.click(within(minimum).getByRole("button", { name: "Save" }));
    expect(await within(row("Minimum")).findByRole("alert")).toHaveTextContent("Something changed at the same time");
    expect(within(row("Minimum")).getByLabelText("Amount")).toHaveValue("150000");
  });

  it("sets the unknown-pay policy on its own", async () => {
    const save = update();
    const { container } = render(<PayControls controls={controls()} update={save} />);
    const policy = await edit("When pay isn't published");
    expect(within(policy).getByRole("radio", { name: /Show them, marked unresolved/ })).toBeChecked();
    await userEvent.click(within(policy).getByRole("radio", { name: /Hide them/ }));
    expect(await violations(container)).toEqual([]);
    await userEvent.click(within(policy).getByRole("button", { name: "Save" }));
    expect(save).toHaveBeenCalledWith([{ kind: "unknown_pay", show: false }], []);
  });

  it("removes the minimum", async () => {
    const save = update();
    render(<PayControls controls={controls()} update={save} />);
    const minimum = await edit("Minimum");
    await userEvent.click(within(minimum).getByRole("button", { name: "Remove minimum" }));
    expect(save).toHaveBeenCalledWith([], ["pref_min"]);
  });
});

describe("Company & team", () => {
  it("keeps team size, company size and stage apart, each must have or nice to have", async () => {
    const save = update();
    const { container } = render(
      <PreferenceEditing>
        <CompanyControls controls={controls()} update={save} />
      </PreferenceEditing>,
    );
    expect(within(row("Team")).getByText("Small team")).toBeInTheDocument();
    expect(within(row("Team")).getByText(/Nice to have/)).toBeInTheDocument();
    expect(within(row("Company size")).getByText("No preference")).toBeInTheDocument();
    expect(within(row("Stage")).getByText("No preference")).toBeInTheDocument();
    await edit("Team");
    const team = screen.getByRole("group", { name: "Small team" });
    expect(within(team).getByRole("radio", { name: "Nice to have" })).toBeChecked();
    await userEvent.click(within(team).getByRole("radio", { name: "Must have" }));
    expect(save).toHaveBeenCalledWith([{ kind: "company", company: "small_team", stance: "require" }], []);
    expect(await violations(container)).toEqual([]);
    await edit("Stage");
    await userEvent.click(within(screen.getByRole("group", { name: "Early-stage" })).getByRole("radio", { name: "Nice to have" }));
    expect(save).toHaveBeenLastCalledWith([{ kind: "company", company: "early_stage", stance: "want" }], []);
    // A company's size is its own row, not the team's.
    expect(within(row("Company size")).queryByRole("radio")).not.toBeInTheDocument();
  });

  it("turns a kind off by removing it", async () => {
    const save = update();
    render(<CompanyControls controls={controls()} update={save} />);
    await edit("Team");
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

  it("says a setting read from words still needs an answer", () => {
    const c = controls();
    const pending: Controls = {
      ...c,
      work: { ...c.work, setup_records: [stated({ id: "pref_remote", clarify: { kind: "importance", value: "remote work", input: { kind: "work_mode", mode: "remote", stance: "want" } } })] },
    };
    render(<WorkControls controls={pending} update={update()} />);
    expect(within(row("Work setup")).getByText("needs your answer")).toHaveClass("nr-inferred");
  });
});

describe("one source of truth", () => {
  it("shows the stored value when it changes elsewhere (the person's words)", () => {
    const c = controls();
    const lisbon: Controls = { ...c, location: { ...c.location, home: "Lisbon, Portugal", home_country: "Portugal", home_basis: "resume" } };
    const { rerender } = render(<LocationControls controls={lisbon} update={update()} />);
    expect(within(row("Where you live")).getByText("Lisbon, Portugal")).toBeInTheDocument();
    expect(within(row("Where you live")).getByText(/from your resume/)).toBeInTheDocument();
    rerender(<LocationControls controls={c} update={update()} />);
    expect(within(row("Where you live")).getByText("Brazil")).toBeInTheDocument();
  });

  it("opens the pay editor on the stored minimum", async () => {
    const c = controls();
    const raised: Controls = {
      ...c,
      pay: { ...c.pay, minimum: [{ ...c.pay.minimum[0]!, amount: 150000, record: stated({ id: "pref_min_2", category: "compensation" }) }] },
    };
    render(<PayControls controls={raised} update={update()} />);
    expect(within(await edit("Minimum")).getByLabelText("Amount")).toHaveValue("150000");
  });
});

describe("pay periods (Codex review #3)", () => {
  for (const period of ["hour", "day", "month", "year"] as const) {
    it(`keeps a USD 100 per ${period} figure exactly when saved unchanged`, async () => {
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
      expect(within(row("Minimum")).getByText(`At least USD 100 per ${period}`)).toBeInTheDocument();
      const minimum = await edit("Minimum");
      expect(within(minimum).getByLabelText("Per")).toHaveValue(period);
      await userEvent.click(within(minimum).getByRole("button", { name: "Save" }));
      expect(save).toHaveBeenCalledWith(
        [{ kind: "compensation", minimum: 100, target: null, currency: "USD", period, applies_to: null }],
        [],
      );
    });
  }

  it("offers every period the API stores", async () => {
    render(<PayControls controls={controls()} update={update()} />);
    const options = within(within(await edit("Target")).getByLabelText("Per")).getAllByRole("option");
    expect(options.map((o) => o.getAttribute("value"))).toEqual(["year", "month", "day", "hour"]);
  });
});

describe("Anywhere (production smoke test)", () => {
  it("is no restriction: no importance, and it says it ranks nothing", async () => {
    const c = controls();
    const anywhere = {
      record: stated({ id: "pref_any", value: "remote roles open anywhere (no geographic restriction)", stance: "wanted", layer: "preference" }),
      place: "Worldwide",
      read_as: "anywhere",
      code: "worldwide",
    };
    render(<LocationControls controls={{ ...c, location: { ...c.location, remote_geography: [anywhere] } }} update={update()} />);
    expect(within(row("Remote roles open to")).getByText("Anywhere (no restriction)")).toBeInTheDocument();
    expect(within(row("Remote roles open to")).queryByText("Nice to have")).not.toBeInTheDocument();
    const scopes = await edit("Remote roles open to");
    expect(within(scopes).getByRole("checkbox", { name: "Anywhere (no restriction)" })).toBeChecked();
    expect(within(scopes).getByText(/doesn't rank remote roles up or down/)).toBeInTheDocument();
  });
});
