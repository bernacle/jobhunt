import { expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

// A fictional LinkedIn export's Positions.csv: the resume's current role,
// and one only LinkedIn has.
const POSITIONS = [
  "Company Name,Title,Description,Location,Started On,Finished On",
  "Acme Payments,Staff Software Engineer,,Remote,Jan 2021,",
  "Initech,Backend Engineer,Maintained the billing API in Go.,,Jan 2015,May 2016",
].join("\n");

/**
 * Evidence beyond the resume (BRU-309): a LinkedIn export and public GitHub
 * repositories feed the same profile, their conclusions wait in the same
 * review, and either source can be taken out again.
 */
test.describe.serial("LinkedIn and GitHub evidence", () => {
  const name = "e2e-sources";

  test.beforeAll(async () => {
    await onboardViaApi(name);
  });

  test("a LinkedIn export adds to the profile without duplicating it", async ({ page }) => {
    await signIn(page, name, "/profile");
    const sources = page.getByRole("region", { name: "Sources" });
    await sources.getByText("Add your LinkedIn export").click();
    await expect(sources.getByText(/Messages, connections and contacts are never opened/)).toBeVisible();
    await sources.getByLabel("Your LinkedIn data export").setInputFiles({
      name: "Positions.csv",
      mimeType: "text/csv",
      buffer: Buffer.from(POSITIONS),
    });
    await sources.getByRole("button", { name: "Import" }).click();
    const summary = sources.getByRole("status").filter({ hasText: "LinkedIn export imported" });
    await expect(summary).toBeVisible();
    await expect(summary).toContainText("1 already in your profile, now also backed by this source");
    await expect(sources.getByText("LinkedIn export", { exact: true })).toBeVisible();
    // One Acme position, backed by both sources; Initech is new.
    const experience = page.getByRole("region", { name: "Experience" });
    await page.reload();
    await expect(experience.getByText("resume + LinkedIn")).toBeVisible();
    await expect(experience.getByText("Initech")).toBeVisible();
    await expectAccessible(page);
  });

  test("GitHub repositories become reviewable evidence", async ({ page }) => {
    await signIn(page, name, "/profile");
    const sources = page.getByRole("region", { name: "Sources" });
    await sources.getByText("Add your GitHub").click();
    await sources.getByLabel("GitHub username or profile URL").fill("analima");
    await sources.getByRole("button", { name: "Import" }).click();
    const summary = sources.getByRole("status").filter({ hasText: "GitHub imported" });
    await expect(summary).toBeVisible();
    await expect(summary).toContainText("Not used: 1 fork");
    await summary.getByRole("link", { name: /^Review \d+ claims?$/ }).click();
    await expect(page.getByRole("heading", { level: 1, name: "Review claims" })).toBeVisible();
    const review = page.getByRole("region", { name: "Needs your review" });
    const showAll = review.getByRole("button", { name: /^Show all/ });
    if (await showAll.count()) await showAll.click();
    // The conclusion is Narrow's, so it waits for the person: confirm it.
    await review.getByRole("button", { name: /^Confirm.*Recent hands-on Go work in public GitHub repositories/ }).click();
    await expect(review.getByText(/Confirmed: it can be used as evidence/)).toBeVisible();
    await expectAccessible(page);
  });

  test("a source can be taken out again", async ({ page }) => {
    await signIn(page, name, "/profile");
    const sources = page.getByRole("region", { name: "Sources" });
    await sources.getByRole("button", { name: "Remove LinkedIn export" }).click();
    await expect(sources.getByRole("group", { name: "Remove LinkedIn export" })).toContainText("so do your confirmations");
    await sources.getByRole("group").getByRole("button", { name: "Remove LinkedIn export" }).click();
    await expect(sources.getByText("LinkedIn export", { exact: true })).toHaveCount(0);
    const experience = page.getByRole("region", { name: "Experience" });
    await expect(experience.getByText("Initech")).toHaveCount(0);
    await expect(experience.getByText("Acme Payments").first()).toBeVisible();
    await expectAccessible(page);
  });
});
