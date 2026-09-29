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

/** The one open editor: a modal sheet titled with the decision. */
function editor() {
  return screen.getByRole("dialog");
}

/** Opens a row's editor with its one action (Edit, Add or Change), and returns it. */
async function edit(name: string) {
  await userEvent.click(within(row(name)).getByRole("button", { name: new RegExp(`^(Edit|Add|Change) ${name}$`, "i") }));
  return editor();
}

async function save(sheet: HTMLElement) {
  await userEvent.click(within(sheet).getByRole("button", { name: "Save" }));
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
  it("shows each setting as its value, its layer and one action, with no editor open", async () => {
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
    // A requirement says so on the overview; an unset row says nothing about layers.
    expect(within(row("Work setup")).getByText("Requirement")).toBeInTheDocument();
    expect(within(row("Minimum")).getByText("Requirement")).toBeInTheDocument();
    expect(within(row("Target")).queryByText(/Requirement|Preference/)).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(screen.queryByRole("radio")).not.toBeInTheDocument();
    expect(screen.queryByRole("textbox")).not.toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });

  it("says a target is a preference, not a requirement", () => {
    const c = controls();
    const withTarget: Controls = {
      ...c,
      pay: { ...c.pay, target: [{ record: stated({ id: "pref_target", stance: "wanted", layer: "preference" }), amount: 180000, currency: "USD", period: "year" }] },
    };
    render(<PayControls controls={withTarget} update={update()} />);
    expect(within(row("Target")).getByText("Preference")).toBeInTheDocument();
    expect(within(row("Minimum")).getByText("Requirement")).toBeInTheDocument();
  });

  it("opens one decision in a focused sheet, leaving the overview as it was", async () => {
    const { container } = render(
      <PreferenceEditing>
        <WorkControls controls={controls()} update={update()} />
        <PayControls controls={controls()} update={update()} />
      </PreferenceEditing>,
    );
    const sheet = await edit("Work setup");
    expect(sheet).toHaveAccessibleName("How do you want to work?");
    expect(within(sheet).getByText("Work setup · Now: Remote only")).toBeInTheDocument();
    expect(within(sheet).getAllByRole("radio")).toHaveLength(5);
    // Only this decision's controls: nothing else is being edited, and the row keeps its value.
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(screen.getAllByRole("radio").every((r) => sheet.contains(r))).toBe(true);
    expect(screen.queryByLabelText("Amount")).not.toBeInTheDocument();
    expect(within(row("Work setup")).getByText("Remote only")).toBeInTheDocument();
    expect(within(row("Work setup")).getByRole("button", { name: "Edit work setup" })).toHaveAttribute("aria-expanded", "true");
    // Focus starts on the current answer.
    expect(within(sheet).getByRole("radio", { name: /Remote only/ })).toHaveFocus();
    expect(await violations(container)).toEqual([]);
  });

  it("keeps one decision open at a time, and Escape cancels back to the row", async () => {
    render(
      <PreferenceEditing>
        <WorkControls controls={controls()} update={update()} />
      </PreferenceEditing>,
    );
    await edit("Work setup");
    await edit("Relocation");
    expect(screen.getAllByRole("dialog")).toHaveLength(1);
    expect(editor()).toHaveAccessibleName("Would you move for a role?");
    expect(within(editor()).getAllByRole("radio")).toHaveLength(3);
    await userEvent.keyboard("{Escape}");
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    await waitFor(() => expect(within(row("Relocation")).getByRole("button", { name: "Edit relocation" })).toHaveFocus());
  });

  it("Cancel discards the draft: nothing is sent and the next edit starts from what is stored", async () => {
    const send = update();
    render(<WorkControls controls={controls()} update={send} />);
    let sheet = await edit("Work setup");
    await userEvent.click(within(sheet).getByRole("radio", { name: /Prefer remote/ }));
    await userEvent.click(within(sheet).getByRole("button", { name: "Cancel" }));
    expect(send).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
    expect(within(row("Work setup")).getByText("Remote only")).toBeInTheDocument();
    sheet = await edit("Work setup");
    expect(within(sheet).getByRole("radio", { name: /Remote only/ })).toBeChecked();
  });

  it("a save that finishes after the person moved on doesn't close the next decision", async () => {
    let finish: (value: { ok: true; data: never }) => void = () => {};
    const slow = vi.fn(() => new Promise<{ ok: true; data: never }>((resolve) => (finish = resolve)));
    render(
      <PreferenceEditing>
        <WorkControls controls={controls()} update={slow} />
      </PreferenceEditing>,
    );
    const setup = await edit("Work setup");
    await userEvent.click(within(setup).getByRole("radio", { name: /Prefer remote/ }));
    await save(setup);
    await userEvent.keyboard("{Escape}");
    await edit("Relocation");
    finish({ ok: true, data: { unchanged: false } as never });
    expect(await within(row("Work setup")).findByText("Saved.")).toBeInTheDocument();
    expect(editor()).toHaveAccessibleName("Would you move for a role?");
  });

  it("titles an unset decision as not set, and offers Add", async () => {
    render(<PayControls controls={controls()} update={update()} />);
    const sheet = await edit("Target");
    expect(sheet).toHaveAccessibleName("What pay are you aiming for?");
    expect(within(sheet).getByText("Target · Not set")).toBeInTheDocument();
    expect(within(sheet).getByLabelText("Amount")).toHaveValue("");
  });
});

describe("Work", () => {
  it("replaces the work setup with one answer, saying the consequence of the chosen one only", async () => {
    const send = update();
    const { container } = render(<WorkControls controls={controls()} update={send} />);
    const setup = await edit("Work setup");
    expect(within(setup).getByRole("radio", { name: /Remote only/ })).toBeChecked();
    expect(within(setup).getByText("Hybrid and on-site roles are left out.")).toBeInTheDocument();
    expect(within(setup).queryByText("Remote roles rank higher. Nothing is left out.")).not.toBeInTheDocument();
    await userEvent.click(within(setup).getByRole("radio", { name: /Prefer remote/ }));
    expect(within(setup).getByText("Remote roles rank higher. Nothing is left out.")).toBeInTheDocument();
    expect(send).not.toHaveBeenCalled();
    expect(await violations(container)).toEqual([]);
    await save(setup);
    expect(send).toHaveBeenCalledWith([{ kind: "work_setup", setup: "prefer_remote" }], []);
    expect(await within(row("Work setup")).findByText("Saved.")).toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("keeps relocation a separate answer, with places when only some", async () => {
    const send = update();
    render(<WorkControls controls={controls()} update={send} />);
    const relocation = await edit("Relocation");
    expect(within(relocation).getByRole("radio", { name: /Not willing to relocate/ })).toBeChecked();
    await userEvent.click(within(relocation).getByRole("radio", { name: /Only to some places/ }));
    await save(relocation);
    expect(send).not.toHaveBeenCalled();
    expect(within(relocation).getByRole("alert")).toHaveTextContent("Name at least one country or region.");
    await userEvent.type(within(relocation).getByLabelText("Countries or regions"), "Portugal, Spain");
    await save(relocation);
    expect(send).toHaveBeenCalledWith([{ kind: "relocation", willing: true, only_to: ["Portugal", "Spain"] }], []);
  });

  it("says when stored work modes are none of the five answers, and asks for one before saving", async () => {
    const c = controls();
    const send = update();
    const custom: Controls = { ...c, work: { ...c.work, setup: "custom", custom: "wanted hybrid, unwanted on-site", setup_layer: "preference" } };
    render(<WorkControls controls={custom} update={send} />);
    expect(within(row("Work setup")).getByText(/From your words: wanted hybrid, unwanted on-site/)).toBeInTheDocument();
    const setup = await edit("Work setup");
    expect(within(setup).getAllByRole("radio").filter((r) => (r as HTMLInputElement).checked)).toHaveLength(0);
    await save(setup);
    expect(within(setup).getByRole("alert")).toHaveTextContent("Choose one of the answers.");
    expect(send).not.toHaveBeenCalled();
  });
});

describe("Location", () => {
  it("reads where the person lives into the remote scopes that include them, and never calls bare Remote global", async () => {
    render(<LocationControls controls={controls()} update={update()} />);
    expect(within(row("Where you live")).getByText("Brazil")).toBeInTheDocument();
    const home = await edit("Where you live");
    expect(home).toHaveAccessibleName("Where do you live?");
    expect(within(home).getByLabelText("Where you live")).toHaveValue("Brazil");
    expect(within(home).getByText(/anywhere, the Americas, Latin America, South America or Brazil include you/)).toBeInTheDocument();
    expect(within(home).getByText(/only says “Remote” doesn't say where/)).toBeInTheDocument();
  });

  it("asks for a place before saving where the person lives", async () => {
    const send = update();
    render(<LocationControls controls={controls()} update={send} />);
    const home = await edit("Where you live");
    await userEvent.clear(within(home).getByLabelText("Where you live"));
    await save(home);
    expect(within(home).getByRole("alert")).toHaveTextContent("Enter where you live");
    expect(send).not.toHaveBeenCalled();
  });

  it("adds where the person may legally work as a draft, saved in one change", async () => {
    const send = update();
    render(<LocationControls controls={controls()} update={send} />);
    expect(within(row("Authorized to work in")).getByText("Not set")).toBeInTheDocument();
    const legal = await edit("Authorized to work in");
    expect(within(legal).getByText(/stays unresolved/)).toBeInTheDocument();
    await userEvent.type(within(legal).getByLabelText("Add a country or region"), "Portugal{Enter}");
    expect(within(legal).getByRole("list", { name: "Authorized to work in" })).toHaveTextContent("Portugal");
    // A place typed but not yet added goes with the rest.
    await userEvent.type(within(legal).getByLabelText("Add a country or region"), "the EU");
    expect(send).not.toHaveBeenCalled();
    await save(legal);
    expect(send).toHaveBeenCalledOnce();
    expect(send).toHaveBeenCalledWith(
      [
        { kind: "authorized_in", place: "Portugal" },
        { kind: "authorized_in", place: "the EU" },
      ],
      [],
    );
  });

  it("removes a stored place only on Save, and not at all on Cancel", async () => {
    const send = update();
    const c = controls();
    const withPlaces: Controls = {
      ...c,
      location: {
        ...c.location,
        authorized_in: [
          { record: stated({ id: "pref_auth_br", value: "authorized in Brazil" }), place: "Brazil", read_as: "Brazil", code: "BR" },
          { record: stated({ id: "pref_auth_x", value: "authorized in Atlantis" }), place: "Atlantis", read_as: null, code: null },
        ],
      },
    };
    render(<LocationControls controls={withPlaces} update={send} />);
    expect(within(row("Authorized to work in")).getByText("Brazil, Atlantis")).toBeInTheDocument();
    let legal = await edit("Authorized to work in");
    expect(within(legal).getByText(/not recognized/)).toBeInTheDocument();
    await userEvent.click(within(legal).getByRole("button", { name: "Remove Atlantis" }));
    await userEvent.click(within(legal).getByRole("button", { name: "Cancel" }));
    expect(send).not.toHaveBeenCalled();
    legal = await edit("Authorized to work in");
    await userEvent.click(within(legal).getByRole("button", { name: "Remove Atlantis" }));
    await save(legal);
    expect(send).toHaveBeenCalledWith([], ["pref_auth_x"]);
  });

  it("changes remote scopes and their importance as one decision", async () => {
    const send = update();
    const c = controls();
    const latam = {
      record: stated({ id: "pref_latam", value: "work in Latin America", stance: "wanted", layer: "preference" }),
      place: "Latin America",
      read_as: "Latin America",
      code: "latam",
    };
    const withScope: Controls = { ...c, location: { ...c.location, remote_geography: [latam] } };
    render(<LocationControls controls={withScope} update={send} />);
    expect(within(row("Remote roles open to")).getByText("Latin America")).toBeInTheDocument();
    expect(within(row("Remote roles open to")).getByText("Preference")).toBeInTheDocument();
    const scopes = await edit("Remote roles open to");
    expect(scopes).toHaveAccessibleName("Which remote regions work for you?");
    expect(within(scopes).getByRole("checkbox", { name: "Latin America (LATAM)" })).toBeChecked();
    await userEvent.click(within(scopes).getByRole("checkbox", { name: "The Americas" }));
    await userEvent.click(within(scopes).getByRole("checkbox", { name: "Latin America (LATAM)" }));
    await userEvent.click(within(scopes).getByRole("radio", { name: "Must have" }));
    expect(send).not.toHaveBeenCalled();
    await save(scopes);
    expect(send).toHaveBeenCalledOnce();
    expect(send).toHaveBeenCalledWith([{ kind: "region", region: "Americas", stance: "require" }], ["pref_latam"]);
  });

  it("applies a new importance to every place kept", async () => {
    const send = update();
    const c = controls();
    const places = ["Brazil", "Portugal"].map((place, i) => ({
      record: stated({ id: `pref_geo_${i}`, value: `work in ${place}`, stance: "wanted", layer: "preference" }),
      place,
      read_as: place,
      code: place.slice(0, 2).toUpperCase(),
    }));
    render(<LocationControls controls={{ ...c, location: { ...c.location, remote_geography: places } }} update={send} />);
    const scopes = await edit("Remote roles open to");
    await userEvent.click(within(scopes).getByRole("radio", { name: "Must have" }));
    await save(scopes);
    expect(send).toHaveBeenCalledWith(
      [
        { kind: "region", region: "Brazil", stance: "require" },
        { kind: "region", region: "Portugal", stance: "require" },
      ],
      [],
    );
  });

  it("closes without a request when nothing changed", async () => {
    const send = update();
    render(<LocationControls controls={controls()} update={send} />);
    await save(await edit("Remote roles open to"));
    expect(send).not.toHaveBeenCalled();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("sets the policy for unclear eligibility", async () => {
    const send = update();
    render(<LocationControls controls={controls()} update={send} />);
    expect(within(row("When eligibility is unclear")).getByText("Show them, marked unresolved")).toBeInTheDocument();
    const policy = await edit("When eligibility is unclear");
    expect(policy).toHaveAccessibleName("Show roles when your eligibility is unclear?");
    expect(within(policy).getByRole("radio", { name: /Show them, marked unresolved/ })).toBeChecked();
    await userEvent.click(within(policy).getByRole("radio", { name: /Only when it's confirmed/ }));
    await save(policy);
    expect(send).toHaveBeenCalledWith([{ kind: "unclear_eligibility", show: false }], []);
  });

  it("keeps many long regions whole on the row and removable one by one in the editor", async () => {
    const c = controls();
    const names = ["Brazil", "Portugal", "Spain", "Mexico", "Colombia", "Argentina", "Uruguay", "the Autonomous Region of the Azores and Madeira"];
    const many = names.map((place, i) => ({
      record: stated({ id: `pref_many_${i}`, value: `work in ${place}`, stance: "wanted", layer: "preference" }),
      place,
      read_as: place,
      code: `C${i}`,
    }));
    render(<LocationControls controls={{ ...c, location: { ...c.location, remote_geography: many } }} update={update()} />);
    expect(within(row("Remote roles open to")).getByText(names.join(", "))).toBeInTheDocument();
    const scopes = await edit("Remote roles open to");
    const list = within(scopes).getByRole("list", { name: "Other places" });
    expect(within(list).getAllByRole("button", { name: /^Remove / })).toHaveLength(names.length);
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
    const minimum = await edit("Minimum");
    expect(within(minimum).getByText("Verified pay below it is left out.")).toBeInTheDocument();
    expect(within(minimum).getByText(/never counts as meeting it/)).toBeInTheDocument();
    expect(within(minimum).getByText(/another currency or period \(never converted\)/)).toBeInTheDocument();
    await userEvent.keyboard("{Escape}");
    expect(within(await edit("Target")).getByText("Changes the order. Leaves nothing out.")).toBeInTheDocument();
  });

  it("never assumes a currency", async () => {
    const send = update();
    const c = controls();
    render(<PayControls controls={{ ...c, pay: { ...c.pay, minimum: [] } }} update={send} />);
    const minimum = await edit("Minimum");
    expect(within(minimum).getByLabelText("Currency")).toHaveValue("");
    await userEvent.type(within(minimum).getByLabelText("Amount"), "140,000");
    await save(minimum);
    expect(send).not.toHaveBeenCalled();
    expect(within(minimum).getByRole("alert")).toHaveTextContent("Narrow never assumes one");
    await userEvent.type(within(minimum).getByLabelText("Currency"), "usd");
    await userEvent.selectOptions(within(minimum).getByLabelText("Per"), "month");
    await save(minimum);
    expect(send).toHaveBeenCalledWith(
      [{ kind: "compensation", minimum: 140000, target: null, currency: "USD", period: "month", applies_to: null }],
      [],
    );
  });

  it("asks for an amount that is a number", async () => {
    const send = update();
    render(<PayControls controls={controls()} update={send} />);
    const target = await edit("Target");
    await userEvent.type(within(target).getByLabelText("Amount"), "a lot");
    await save(target);
    expect(within(target).getByRole("alert")).toHaveTextContent("Enter the amount as a number");
    expect(send).not.toHaveBeenCalled();
  });

  it("keeps an error beside the editor, and the old value in place", async () => {
    const send = vi.fn(async () => ({ ok: false as const, code: "conflict", title: "Something changed at the same time", message: "Nothing was lost. Try that again." }));
    render(<PayControls controls={controls()} update={send} />);
    const minimum = await edit("Minimum");
    await userEvent.clear(within(minimum).getByLabelText("Amount"));
    await userEvent.type(within(minimum).getByLabelText("Amount"), "150000");
    await save(minimum);
    expect(await within(minimum).findByRole("alert")).toHaveTextContent("Something changed at the same time");
    expect(within(minimum).getByLabelText("Amount")).toHaveValue("150000");
    expect(screen.getByRole("dialog")).toBe(minimum);
    expect(within(row("Minimum")).getByText("At least USD 140,000 per year")).toBeInTheDocument();
  });

  it("sets the unknown-pay policy on its own", async () => {
    const send = update();
    const { container } = render(<PayControls controls={controls()} update={send} />);
    const policy = await edit("When pay isn't published");
    expect(within(policy).getByRole("radio", { name: /Show them, marked unresolved/ })).toBeChecked();
    await userEvent.click(within(policy).getByRole("radio", { name: /Hide them/ }));
    expect(await violations(container)).toEqual([]);
    await save(policy);
    expect(send).toHaveBeenCalledWith([{ kind: "unknown_pay", show: false }], []);
  });

  it("removes the minimum", async () => {
    const send = update();
    render(<PayControls controls={controls()} update={send} />);
    const minimum = await edit("Minimum");
    await userEvent.click(within(minimum).getByRole("button", { name: "Remove minimum" }));
    expect(send).toHaveBeenCalledWith([], ["pref_min"]);
    expect(await within(row("Minimum")).findByText("Removed.")).toBeInTheDocument();
  });

  it("removes a figure for one arrangement with the rest of the change", async () => {
    const send = update();
    const c = controls();
    const contract: Controls = {
      ...c,
      pay: {
        ...c.pay,
        minimum: [
          ...c.pay.minimum,
          { record: stated({ id: "pref_min_contract", category: "compensation" }), amount: 6000, currency: "EUR", period: "month", applies_to: "contract" },
        ],
      },
    };
    render(<PayControls controls={contract} update={send} />);
    expect(within(row("Minimum")).getByText(/EUR 6,000 per month \(contract only\)/)).toBeInTheDocument();
    const minimum = await edit("Minimum");
    await userEvent.click(within(minimum).getByRole("button", { name: "Remove EUR 6,000 per month (contract only)" }));
    expect(within(minimum).queryByText("EUR 6,000 per month (contract only)")).not.toBeInTheDocument();
    expect(send).not.toHaveBeenCalled();
    await save(minimum);
    expect(send).toHaveBeenCalledWith(
      [{ kind: "compensation", minimum: 140000, target: null, currency: "USD", period: "year", applies_to: null }],
      ["pref_min_contract"],
    );
  });
});

describe("Company & team", () => {
  it("keeps team size, company size and stage apart, each changed as one decision", async () => {
    const send = update();
    const { container } = render(
      <PreferenceEditing>
        <CompanyControls controls={controls()} update={send} />
      </PreferenceEditing>,
    );
    expect(within(row("Team")).getByText("Small team")).toBeInTheDocument();
    expect(within(row("Team")).getByText(/Nice to have/)).toBeInTheDocument();
    expect(within(row("Company size")).getByText("No preference")).toBeInTheDocument();
    expect(within(row("Stage")).getByText("No preference")).toBeInTheDocument();
    const team = await edit("Team");
    expect(team).toHaveAccessibleName("What size of team suits you?");
    const small = within(team).getByRole("group", { name: "Small team" });
    expect(within(small).getByRole("radio", { name: "Nice to have" })).toBeChecked();
    await userEvent.click(within(small).getByRole("radio", { name: "Must have" }));
    // What a must have does when a posting doesn't say, only once one is chosen.
    expect(within(team).getByText(/a must have stays unresolved/)).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
    expect(send).not.toHaveBeenCalled();
    await save(team);
    expect(send).toHaveBeenCalledWith([{ kind: "company", company: "small_team", stance: "require" }], []);

    const stage = await edit("Stage");
    // A company's size is its own decision, not the team's.
    expect(within(stage).queryByRole("group", { name: "Small team" })).not.toBeInTheDocument();
    expect(within(stage).queryByText(/a must have stays unresolved/)).not.toBeInTheDocument();
    await userEvent.click(within(within(stage).getByRole("group", { name: "Early-stage" })).getByRole("radio", { name: "Nice to have" }));
    await userEvent.click(within(within(stage).getByRole("group", { name: "Scale-up" })).getByRole("radio", { name: "Avoid" }));
    await save(stage);
    expect(send).toHaveBeenLastCalledWith(
      [
        { kind: "company", company: "early_stage", stance: "want" },
        { kind: "company", company: "scaleup", stance: "avoid" },
      ],
      [],
    );
  });

  it("turns a kind off by removing it", async () => {
    const send = update();
    render(<CompanyControls controls={controls()} update={send} />);
    const team = await edit("Team");
    await userEvent.click(within(within(team).getByRole("group", { name: "Small team" })).getByRole("radio", { name: "Off" }));
    await save(team);
    expect(send).toHaveBeenCalledWith([], ["pref_team"]);
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
      const send = update();
      const c = controls();
      const figure: Controls = {
        ...c,
        pay: {
          ...c.pay,
          minimum: [{ record: stated({ id: "pref_min", category: "compensation" }), amount: 100, currency: "USD", period }],
        },
      };
      render(<PayControls controls={figure} update={send} />);
      expect(within(row("Minimum")).getByText(`At least USD 100 per ${period}`)).toBeInTheDocument();
      const minimum = await edit("Minimum");
      expect(within(minimum).getByLabelText("Per")).toHaveValue(period);
      await save(minimum);
      expect(send).toHaveBeenCalledWith(
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
