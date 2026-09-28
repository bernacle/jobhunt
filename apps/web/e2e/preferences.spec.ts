import { type Page, expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

/**
 * BRU-308: the structured settings and the person's words are one set of
 * preferences. A sentence fills in the settings; a setting changed
 * directly replaces what the sentence set, and the sentence stays as
 * written. BRU-313: the page is a summary of those decisions, each row
 * edited in place, one at a time.
 */
const WORDS = "remote from Brazil, at least USD 140k, prefer small teams";

function row(page: Page, name: string) {
  return page.getByRole("group", { name, exact: true });
}

/** Opens a row's editor, chooses an answer, saves, and waits for the API. */
async function choose(page: Page, name: string, answer: string) {
  await row(page, name).getByRole("button", { name: new RegExp(`^(Edit|Add|Change) ${name}$`, "i") }).click();
  await row(page, name).getByText(answer, { exact: true }).click();
  await row(page, name).getByRole("button", { name: "Save" }).click();
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

    await page.getByRole("button", { name: "Add in your words" }).click();
    await page.getByLabel("Describe what you're looking for").fill(WORDS);
    await page.getByRole("button", { name: "Update preferences" }).click();
    await expect(page.getByRole("heading", { name: "Understood" })).toBeVisible();

    await expect(row(page, "Work setup").getByText("Remote only")).toBeVisible();
    await expect(row(page, "Where you live").getByText("Brazil")).toBeVisible();
    await expect(row(page, "Minimum").getByText("At least USD 140,000 per year")).toBeVisible();
    await expect(row(page, "Team")).toContainText("Small team · Nice to have");
    await expect(row(page, "Company size").getByText("No preference")).toBeVisible();
    // No editor is open until the person asks for one.
    await expect(page.getByRole("radio")).toHaveCount(0);
    await row(page, "Where you live").getByRole("button", { name: "Edit where you live" }).click();
    await expect(row(page, "Where you live").getByLabel("Where you live")).toHaveValue("Brazil");
    await expect(row(page, "Where you live").getByText(/Latin America.* include you/)).toBeVisible();
    await page.keyboard.press("Escape");
    await expectAccessible(page);
  });

  test("settings change directly, without rewriting the sentence", async ({ page }) => {
    await signIn(page, name, "/preferences");
    await choose(page, "Work setup", "Prefer remote");
    await choose(page, "Relocation", "Not willing to relocate");
    await choose(page, "When pay isn't published", "Hide them");
    await row(page, "Team").getByRole("button", { name: "Edit team" }).click();
    await page.getByRole("group", { name: "Small team", exact: true }).getByText("Must have").click();
    await expect(page.getByRole("group", { name: "Small team", exact: true }).getByRole("radio", { name: "Must have" })).toBeChecked();
    await row(page, "Team").getByRole("button", { name: "Done" }).click();

    // The currency is never filled in for the person.
    const target = row(page, "Target");
    await target.getByRole("button", { name: "Add target" }).click();
    await target.getByLabel("Amount").fill("180000");
    await target.getByRole("button", { name: "Save" }).click();
    await expect(target.getByRole("alert")).toContainText("never assumes");
    await target.getByLabel("Currency").fill("USD");
    await target.getByRole("button", { name: "Save" }).click();
    await expect(target.getByRole("status")).toHaveText(/Saved|Already in effect/);

    await page.reload();
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
      await expect(row(page, "Work setup")).toBeVisible();
      await expectAccessible(page);
      await row(page, "Minimum").getByRole("button", { name: "Edit minimum" }).click();
      await expectAccessible(page);
    });
  }
});
