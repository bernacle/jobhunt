import { type Page, expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

/**
 * BRU-321: Preferences is what Narrow understands about the person. One
 * sentence becomes a short summary to confirm or correct; practical
 * constraints are kept apart (their settings one tap away, under Edit
 * constraints); every other structured setting is under Fine-tune.
 *
 * BRU-308: the structured settings and the person's words are one set of
 * preferences. A sentence fills in the settings; a setting changed
 * directly replaces what the sentence set, and the sentence stays as
 * written. BRU-313: the page is a summary of those decisions. BRU-314:
 * each one is changed alone, in a focused sheet, as a draft saved (or
 * cancelled) in one step; the overview keeps its shape.
 */
const WORDS = "remote from Brazil, at least USD 140k, prefer small teams";

function row(page: Page, name: string) {
  return page.getByRole("group", { name, exact: true });
}

/** Opens the practical constraints' settings and the Fine-tune section. */
async function openSettings(page: Page) {
  await page.locator("summary").filter({ hasText: "Edit constraints" }).click();
  await page.locator("summary").filter({ hasText: "Fine-tune" }).click();
}

/** Opens a row's editor (the one open sheet) and returns it. */
async function edit(page: Page, name: string) {
  await row(page, name).getByRole("button", { name: new RegExp(`^(Edit|Add|Change) ${name}$`, "i") }).click();
  const sheet = page.getByRole("dialog");
  await expect(sheet).toBeVisible();
  return sheet;
}

/** Opens a row's editor, chooses an answer, saves, and waits for the API. */
async function choose(page: Page, name: string, answer: string) {
  const sheet = await edit(page, name);
  await sheet.getByText(answer, { exact: true }).click();
  await sheet.getByRole("button", { name: "Save" }).click();
  await expect(sheet).toBeHidden();
  await expect(row(page, name).getByRole("status")).toHaveText(/Saved|Already in effect/);
}

test.describe.serial("Preferences", () => {
  const name = "e2e-prefs";

  test("words fill in the settings, shown as a summary", async ({ page }) => {
    await onboardViaApi(name);
    await signIn(page, name, "/preferences");
    // The layers are taught once, on demand.
    await page.getByRole("button", { name: "How preferences work" }).click();
    const how = page.getByRole("dialog", { name: "How preferences work" });
    await expect(how.getByText(/^Requirement/)).toBeVisible();
    await expect(how.getByText(/^Preference/)).toBeVisible();
    await expect(how.getByText("Learned", { exact: true })).toBeVisible();
    await page.keyboard.press("Escape");

    // An existing profile that never chose roles is asked, compactly, and
    // nothing else waits for it.
    await expect(page.getByRole("region", { name: "What kind of role are you looking for?" })).toBeVisible();
    // The words onboarding stored are the starting point; describe anew.
    await expect(page.getByText("What you told Narrow before:")).toBeVisible();
    await page.getByRole("button", { name: "Describe it again" }).click();
    await page.getByRole("textbox", { name: "Anything else you care about?" }).fill(WORDS);
    await page.getByRole("button", { name: "Save" }).click();
    const understood = page.getByRole("region", { name: "What Narrow understands" });
    await expect(understood.getByRole("list", { name: "What you want" }).getByText("Small technical teams")).toBeVisible();
    // Practical constraints are never taste.
    await expect(page.getByRole("list", { name: "Your practical constraints" }).getByText("Remote only")).toBeVisible();
    await expect(understood.getByRole("list", { name: "What you want" }).getByText(/remote/i)).toHaveCount(0);

    await openSettings(page);

    await expect(row(page, "Work setup").getByText("Remote only")).toBeVisible();
    await expect(row(page, "Where you live").getByText("Brazil")).toBeVisible();
    await expect(row(page, "Minimum").getByText("At least USD 140,000 per year")).toBeVisible();
    await expect(row(page, "Team")).toContainText("Small team · Nice to have");
    await expect(row(page, "Company size").getByText("No preference")).toBeVisible();
    // No editor is open until the person asks for one.
    await expect(page.getByRole("radio")).toHaveCount(0);
    const home = await edit(page, "Where you live");
    await expect(home).toHaveAccessibleName("Where do you live?");
    await expect(home.getByLabel("Where you live")).toHaveValue("Brazil");
    await expect(home.getByLabel("Where you live")).toBeFocused();
    await expect(home.getByText(/Latin America.* include you/)).toBeVisible();
    // Nothing outside the sheet can be changed meanwhile.
    await expect(page.getByRole("radio")).toHaveCount(0);
    await page.keyboard.press("Escape");
    await expect(home).toBeHidden();
    await expect(row(page, "Where you live").getByRole("button", { name: "Edit where you live" })).toBeFocused();
    await expectAccessible(page);
  });

  test("the summary is confirmed or corrected, and corrections stick", async ({ page }) => {
    await signIn(page, name, "/preferences");
    const understood = page.getByRole("region", { name: "What Narrow understands" });
    await expect(understood.getByText("Narrow's reading · check it")).toBeVisible();
    await understood.getByRole("button", { name: "Edit" }).click();
    const sheet = page.getByRole("dialog", { name: "What Narrow understands" });
    await sheet.getByRole("button", { name: "Doesn't matter (Small technical teams)" }).click();
    await expect(sheet.getByText("Small technical teams: doesn't matter").first()).toBeVisible();
    await sheet.getByLabel("Add one sentence").fill("I'd love developer tooling");
    await sheet.getByRole("button", { name: "Add", exact: true }).click();
    // A kind of role in their words becomes what they're looking for, and says so.
    await expect(sheet.getByRole("status").filter({ hasText: "Added Developer tooling to the kinds of role you're looking for." })).toBeVisible();
    await expectAccessible(page);
    await page.keyboard.press("Escape");
    await understood.getByRole("button", { name: "Looks right" }).click();
    await expect(understood.getByText("You confirmed this")).toBeVisible();
    // Reading the same words again keeps every decision.
    await understood.locator("summary").filter({ hasText: "How Narrow read this" }).click();
    await Promise.all([
      page.waitForResponse((r) => r.request().method() === "POST" && r.url().includes("/preferences")),
      understood.getByRole("button", { name: "Read my words again" }).click(),
    ]);
    await page.reload();
    // A kind of role in their own words is their choice of role.
    await expect(page.getByRole("region", { name: "What you're looking for" }).getByText("Developer tooling")).toBeVisible();
    await expect(understood.getByRole("list", { name: "What you want" }).getByText("Small technical teams")).toHaveCount(0);
  });

  test("the kinds of role are chosen in a sheet, changed later, and Cancel keeps them", async ({ page }) => {
    await signIn(page, name, "/preferences");
    const section = page.getByRole("region", { name: "What you're looking for" });
    await section.getByRole("button", { name: /^Change/ }).click();
    const sheet = page.getByRole("dialog", { name: "What kind of role are you looking for?" });
    await expect(sheet.getByRole("checkbox", { name: "Developer tooling" })).toBeChecked();
    await expect(sheet.getByRole("checkbox", { name: "Developer tooling" })).toBeFocused();
    // Keyboard only: Tab to the next role, Space to choose it.
    await page.keyboard.press("Tab");
    await expect(sheet.getByRole("checkbox", { name: "SRE" })).toBeFocused();
    await page.keyboard.press("Space");
    await expect(sheet.getByRole("checkbox", { name: "SRE" })).toBeChecked();
    await sheet.getByRole("checkbox", { name: "Backend" }).check();
    await expectAccessible(page);
    // Cancel keeps what was there.
    await sheet.getByRole("button", { name: "Cancel" }).click();
    await expect(sheet).toBeHidden();
    await expect(section.getByText("Developer tooling", { exact: true })).toBeVisible();
    // Save replaces it in one step, with a long title of their own.
    await section.getByRole("button", { name: /^Change/ }).click();
    await sheet.getByRole("checkbox", { name: "Developer tooling" }).uncheck();
    await sheet.getByRole("checkbox", { name: "Backend" }).check();
    await sheet.getByRole("checkbox", { name: "Platform" }).check();
    await sheet.getByRole("checkbox", { name: "Product engineering" }).check();
    // Three is the most: the rest wait.
    await expect(sheet.getByRole("checkbox", { name: "Mobile" })).toBeDisabled();
    const title = "Infrastructure-focused Product Engineer for developer platforms";
    await sheet.getByLabel(/A title in your words/).fill(title);
    await sheet.getByRole("button", { name: "Save" }).click();
    await expect(sheet).toBeHidden();
    await expect(section.getByText("Backend · Platform · Product engineering")).toBeVisible();
    await expect(section.getByText(title)).toBeVisible();
    await page.reload();
    await expect(section.getByText("Backend · Platform · Product engineering")).toBeVisible();
    // Nothing internal on the page by default (provenance is behind "How Narrow read this").
    expect(await page.locator("main").innerText()).not.toMatch(/work_shape|confidence|origin|weight/);
    await expectAccessible(page);
  });

  test("settings change directly, without rewriting the sentence", async ({ page }) => {
    await signIn(page, name, "/preferences");
    await openSettings(page);
    await choose(page, "Work setup", "Prefer remote");
    await choose(page, "Relocation", "Not willing to relocate");
    await choose(page, "When pay isn't published", "Hide them");
    const team = await edit(page, "Team");
    await team.getByRole("group", { name: "Small team", exact: true }).getByText("Must have").click();
    await expect(team.getByRole("group", { name: "Small team", exact: true }).getByRole("radio", { name: "Must have" })).toBeChecked();
    await team.getByRole("button", { name: "Save" }).click();
    await expect(row(page, "Team").getByRole("status")).toHaveText(/Saved|Already in effect/);

    // Cancel changes nothing.
    const scope = await edit(page, "Remote roles open to");
    await scope.getByText("The Americas", { exact: true }).click();
    await scope.getByRole("button", { name: "Cancel" }).click();
    await expect(row(page, "Remote roles open to").getByText("The Americas")).toHaveCount(0);

    // The currency is never filled in for the person.
    const target = await edit(page, "Target");
    await target.getByLabel("Amount").fill("180000");
    await target.getByRole("button", { name: "Save" }).click();
    await expect(target.getByRole("alert")).toContainText("never assumes");
    await target.getByLabel("Currency").fill("USD");
    await target.getByRole("button", { name: "Save" }).click();
    await expect(target).toBeHidden();
    await expect(row(page, "Target").getByRole("status")).toHaveText(/Saved|Already in effect/);

    await page.reload();
    await openSettings(page);
    await expect(row(page, "Work setup").getByText("Prefer remote")).toBeVisible();
    await expect(row(page, "Relocation").getByText("Not willing to relocate")).toBeVisible();
    await expect(row(page, "When pay isn't published").getByText("Hide them")).toBeVisible();
    await expect(row(page, "Target").getByText("Around USD 180,000 per year")).toBeVisible();
    await expect(row(page, "Team")).toContainText("Small team · Must have");
    // The sentence is kept as written, and the overview agrees with the settings.
    await expect(page.getByRole("region", { name: "In your words" }).getByText(WORDS)).toBeVisible();
    await page.getByRole("button", { name: "Review all" }).click();
    await expect(page.getByRole("dialog", { name: "What Narrow uses" }).getByText("Want: remote work")).toBeVisible();
  });

  for (const scheme of ["light", "dark"] as const) {
    test(`Preferences are accessible in the ${scheme} theme`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await signIn(page, name, "/preferences");
      await expect(page.getByRole("region", { name: "What Narrow understands" })).toBeVisible();
      await expectAccessible(page);
      await openSettings(page);
      await expect(row(page, "Work setup")).toBeVisible();
      await expectAccessible(page);
      await edit(page, "Minimum");
      await expectAccessible(page);
    });
  }
});
