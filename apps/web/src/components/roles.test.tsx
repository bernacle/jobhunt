import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { TasteAction, TasteProfileView, TasteUpdateResult } from "@/lib/api-types";

import { violations } from "../../test/axe";
import { roles, tasteProfile } from "../../test/fixtures";
import { RolesForm, TargetRoles } from "./roles";

const QUESTION = "What kind of role are you looking for?";

function answer(profile: TasteProfileView, changed = true): { ok: true; data: TasteUpdateResult } {
  return { ok: true, data: { action: "set_roles", changed, interpreted: false, profile } };
}

/** A review that answers with the roles the action chose. */
function review() {
  return vi.fn(async (action: TasteAction) => {
    const chosen = action.action === "set_roles" ? action.roles : [];
    const title = action.action === "set_roles" ? action.title : null;
    return answer(tasteProfile({ roles: roles(chosen, { title }) }));
  });
}

function chip(scope: HTMLElement, name: string) {
  return within(scope).getByRole("checkbox", { name });
}

describe("TargetRoles on Preferences", () => {
  it("asks the question, compactly, until it is answered", async () => {
    const { container } = render(<TargetRoles profile={tasteProfile()} review={review()} />);
    expect(screen.getByRole("heading", { name: QUESTION })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose roles" })).toBeInTheDocument();
    // Nothing of the rest of the page waits for it, and nothing internal shows.
    expect(container.textContent).not.toMatch(/work_shape|confidence|stated|weight/);
    expect(await violations(container)).toEqual([]);
  });

  it("chooses one role in a sheet, saved in one step", async () => {
    const act = review();
    render(<TargetRoles profile={tasteProfile()} review={act} />);
    await userEvent.click(screen.getByRole("button", { name: "Choose roles" }));
    const sheet = screen.getByRole("dialog", { name: QUESTION });
    // Their experience is a hint, not a choice.
    expect(within(sheet).getByText(/Your experience shows Backend, Platform/)).toBeInTheDocument();
    expect(chip(sheet, "Backend")).not.toBeChecked();
    await userEvent.click(chip(sheet, "Backend"));
    expect(act).not.toHaveBeenCalled();
    await userEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(act).toHaveBeenCalledOnce();
    expect(act).toHaveBeenCalledWith({ action: "set_roles", roles: ["backend"], title: null });
    await waitFor(() => expect(screen.queryByRole("dialog")).not.toBeInTheDocument());
    expect(screen.getByRole("heading", { name: "What you're looking for" })).toBeInTheDocument();
    expect(screen.getByText("Backend")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Saved.");
  });

  it("shows several roles and a title, and changes or removes them", async () => {
    const act = review();
    const profile = tasteProfile({ roles: roles(["backend", "platform", "product"], { title: "Infrastructure-focused Product Engineer" }) });
    const { container } = render(<TargetRoles profile={profile} review={act} />);
    expect(screen.getByText("Backend · Platform · Product engineering")).toBeInTheDocument();
    expect(screen.getByText("Infrastructure-focused Product Engineer")).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
    await userEvent.click(screen.getByRole("button", { name: /^Change/ }));
    const sheet = screen.getByRole("dialog", { name: QUESTION });
    expect(chip(sheet, "Backend")).toBeChecked();
    // At the limit, only chosen roles can change.
    expect(chip(sheet, "Mobile")).toBeDisabled();
    expect(within(sheet).getByText(/3 of 3 chosen/)).toBeInTheDocument();
    await userEvent.click(chip(sheet, "Product engineering"));
    expect(chip(sheet, "Mobile")).toBeEnabled();
    await userEvent.click(chip(sheet, "Infrastructure"));
    const title = within(sheet).getByLabelText(/A title in your words/);
    await userEvent.clear(title);
    await userEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(act).toHaveBeenCalledWith({ action: "set_roles", roles: ["backend", "platform", "infrastructure"], title: null });
    expect(await screen.findByText("Backend · Platform · Infrastructure")).toBeInTheDocument();
  });

  it("Cancel and Escape save nothing", async () => {
    const act = review();
    render(<TargetRoles profile={tasteProfile({ roles: roles(["backend"]) })} review={act} />);
    await userEvent.click(screen.getByRole("button", { name: /^Change/ }));
    let sheet = screen.getByRole("dialog", { name: QUESTION });
    await userEvent.click(chip(sheet, "Platform"));
    await userEvent.click(within(sheet).getByRole("button", { name: "Cancel" }));
    expect(act).not.toHaveBeenCalled();
    expect(screen.getByText("Backend")).toBeInTheDocument();
    // Opened again: the draft is gone.
    await userEvent.click(screen.getByRole("button", { name: /^Change/ }));
    sheet = screen.getByRole("dialog", { name: QUESTION });
    expect(chip(sheet, "Platform")).not.toBeChecked();
    await userEvent.keyboard("{Escape}");
    expect(act).not.toHaveBeenCalled();
  });

  it("is keyboard-only: Tab to a role, Space to choose it", async () => {
    const act = review();
    render(<TargetRoles profile={tasteProfile()} review={act} />);
    await userEvent.click(screen.getByRole("button", { name: "Choose roles" }));
    const sheet = screen.getByRole("dialog", { name: QUESTION });
    // Focus starts on the first role.
    expect(chip(sheet, "Backend")).toHaveFocus();
    await userEvent.keyboard(" ");
    await userEvent.tab();
    expect(chip(sheet, "Platform")).toHaveFocus();
    await userEvent.keyboard(" ");
    // On to Save, past the other roles and the title.
    for (let i = 0; i < 20 && document.activeElement !== within(sheet).getByRole("button", { name: "Save" }); i++) await userEvent.tab();
    await userEvent.keyboard("{Enter}");
    expect(act).toHaveBeenCalledWith({ action: "set_roles", roles: ["backend", "platform"], title: null });
  });

  it("says why nothing was saved", async () => {
    const failing = vi.fn(async () => ({ ok: false as const, code: "invalid_arguments", title: "That didn't work", message: "choose at most 3 kinds of role" }));
    render(<TargetRoles profile={tasteProfile({ roles: roles(["backend"]) })} review={failing} />);
    await userEvent.click(screen.getByRole("button", { name: /^Change/ }));
    const sheet = screen.getByRole("dialog", { name: QUESTION });
    // Nothing chosen and no title: said before anything is sent.
    await userEvent.click(chip(sheet, "Backend"));
    await userEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(within(sheet).getByRole("alert")).toHaveTextContent("Choose at least one kind of role");
    expect(failing).not.toHaveBeenCalled();
    await userEvent.click(chip(sheet, "Data"));
    // A change answers the error.
    expect(within(sheet).queryByRole("alert")).not.toBeInTheDocument();
    await userEvent.click(within(sheet).getByRole("button", { name: "Save" }));
    expect(await within(sheet).findByRole("alert")).toHaveTextContent("choose at most 3 kinds of role");
    expect(screen.getByRole("dialog")).toBeInTheDocument();
  });
});

describe("RolesForm in onboarding", () => {
  it("answers in seconds: chips, an optional long title, Continue", async () => {
    const act = review();
    const onSaved = vi.fn();
    const { container } = render(<RolesForm roles={roles()} review={act} onSaved={onSaved} />);
    const form = screen.getByRole("form", { name: QUESTION });
    await userEvent.click(chip(form, "Backend"));
    await userEvent.click(chip(form, "Platform"));
    const long = "Infrastructure-focused Product Engineer for developer platforms at early-stage startups";
    await userEvent.type(within(form).getByLabelText(/A title in your words/), long);
    // The title is capped where the API caps it.
    expect(within(form).getByLabelText(/A title in your words/)).toHaveValue(long.slice(0, 80));
    await userEvent.click(within(form).getByRole("button", { name: "Continue" }));
    expect(act).toHaveBeenCalledWith({ action: "set_roles", roles: ["backend", "platform"], title: long.slice(0, 80) });
    await waitFor(() => expect(onSaved).toHaveBeenCalled());
    expect(await violations(container)).toEqual([]);
  });
});
