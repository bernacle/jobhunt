import { type Page, expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

/**
 * BRU-308: the structured settings and the person's words are one set of
 * preferences. A sentence fills in the settings; a setting changed
 * directly replaces what the sentence set, and the sentence stays as
 * written.
 */
const WORDS = "remote from Brazil, at least USD 140k, prefer small teams";

function row(page: Page, name: string) {
  return page.getByRole("group", { name, exact: true });
}

async function saved(page: Page, name: string) {
  await expect(row(page, name).getByRole("status")).toHaveText(/Saved|Already in effect/);
}

test.describe.serial("Preferences", () => {
  const name = "e2e-prefs";

  test("words fill in the settings, in their layers", async ({ page }) => {
    await onboardViaApi(name);
    await signIn(page, name, "/preferences");
    const legend = page.getByLabel("How Narrow uses what you tell it");
    await expect(legend.getByText("Requirement", { exact: true })).toBeVisible();
    await expect(legend.getByText("Preference", { exact: true })).toBeVisible();
    await expect(legend.getByText("Learned", { exact: true })).toBeVisible();

    await page.getByLabel("Describe what you're looking for").fill(WORDS);
    await page.getByRole("button", { name: "Update preferences" }).click();
    await expect(page.getByRole("heading", { name: "Understood" })).toBeVisible();

    await expect(row(page, "Work setup").getByRole("radio", { name: /Remote only/ })).toBeChecked();
    await expect(row(page, "Work setup").getByText("Requirement", { exact: true })).toBeVisible();
    await expect(row(page, "Where you live").getByLabel("Where you live")).toHaveValue("Brazil");
    await expect(row(page, "Where you live").getByText(/Latin America.* include you/)).toBeVisible();
    await expect(row(page, "Minimum").getByText("At least USD 140,000 per year")).toBeVisible();
    const team = page.getByRole("group", { name: "Small team", exact: true });
    await expect(team.getByRole("radio", { name: "Nice to have" })).toBeChecked();
    await expect(page.getByRole("group", { name: "Small company", exact: true }).getByRole("radio", { name: "Off" })).toBeChecked();
    await expectAccessible(page);
  });

  test("settings change directly, without rewriting the sentence", async ({ page }) => {
    await signIn(page, name, "/preferences");
    await row(page, "Work setup").getByText("Prefer remote").click();
    await saved(page, "Work setup");
    await row(page, "Relocation").getByText("Not willing to relocate").click();
    await saved(page, "Relocation");
    await row(page, "When pay isn't published").getByText("Hide them").click();
    await saved(page, "When pay isn't published");
    await page.getByRole("group", { name: "Small team", exact: true }).getByText("Must have").click();
    await expect(page.getByRole("group", { name: "Small team", exact: true }).getByRole("radio", { name: "Must have" })).toBeChecked();

    // The currency is never filled in for the person.
    const target = row(page, "Target");
    await target.getByLabel("Amount").fill("180000");
    await target.getByRole("button", { name: "Set" }).click();
    await expect(target.getByRole("alert")).toContainText("never assumes");
    await target.getByLabel("Currency").fill("USD");
    await target.getByRole("button", { name: "Set" }).click();
    await saved(page, "Target");

    await page.reload();
    await expect(row(page, "Work setup").getByRole("radio", { name: /Prefer remote/ })).toBeChecked();
    await expect(row(page, "Work setup").getByText("Preference", { exact: true })).toBeVisible();
    await expect(row(page, "Relocation").getByRole("radio", { name: /Not willing to relocate/ })).toBeChecked();
    await expect(row(page, "When pay isn't published").getByRole("radio", { name: "Hide them" })).toBeChecked();
    await expect(row(page, "Target").getByText("Around USD 180,000 per year")).toBeVisible();
    const team = page.getByRole("group", { name: "Small team", exact: true });
    await expect(team.getByRole("radio", { name: "Must have" })).toBeChecked();
    await expect(team.getByText("Requirement", { exact: true })).toBeVisible();
    // The sentence is kept as written, and the overview agrees with the settings.
    await expect(page.getByRole("region", { name: "In your words" }).getByText(WORDS)).toBeVisible();
    await expect(page.getByRole("row", { name: /^Location and work/ }).getByText("Want: remote work")).toBeVisible();
  });

  for (const scheme of ["light", "dark"] as const) {
    test(`Preferences are accessible in the ${scheme} theme`, async ({ page }) => {
      await page.emulateMedia({ colorScheme: scheme });
      await signIn(page, name, "/preferences");
      await expect(row(page, "Work setup")).toBeVisible();
      await expectAccessible(page);
    });
  }
});
