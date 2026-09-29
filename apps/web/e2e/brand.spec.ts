import { expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

/**
 * Narrow, not JobHunt, on every page a person sees; tiers in words, never
 * scores; and both themes accessible.
 */
test("every page is Narrow, in words, not scores", async ({ page }) => {
  await onboardViaApi("e2e-brand");
  await signIn(page, "e2e-brand");
  await expect(page).toHaveTitle("Today · Narrow");
  const first = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  const detail = (await first.getByRole("heading", { level: 2 }).getByRole("link").getAttribute("href"))!;
  for (const path of ["/today", detail, "/applications", "/preferences", "/profile", "/settings"]) {
    await page.goto(path);
    await expect(page.getByRole("heading", { level: 1 })).toBeVisible();
    const text = await page.locator("body").innerText();
    expect(text, path).not.toMatch(/JobHunt/);
    expect(text, path).not.toMatch(/\d+\s?%\s*(match|fit)|good fit|stretch/i);
  }
  await page.goto("/signin");
  expect(await page.locator("body").innerText()).not.toMatch(/JobHunt|Pricing/);
});

for (const scheme of ["dark", "light"] as const) {
  test(`Today and the opportunity page are accessible in the ${scheme} theme`, async ({ page }) => {
    await page.emulateMedia({ colorScheme: scheme });
    await onboardViaApi(`e2e-theme-${scheme}`);
    await signIn(page, `e2e-theme-${scheme}`);
    const background = await page.evaluate(() => getComputedStyle(document.body).backgroundColor);
    expect(background).toBe(scheme === "dark" ? "rgb(12, 13, 14)" : "rgb(247, 247, 248)");
    await expectAccessible(page);
    await page.getByRole("list", { name: "Recommendations" }).getByRole("article").first().getByRole("heading", { level: 2 }).getByRole("link").click();
    await expect(page.getByRole("button", { name: "Full reasoning" })).toBeVisible();
    await expectAccessible(page);
    // Open, the evidence is accessible too.
    await page.getByRole("button", { name: "Checks for eligibility" }).click();
    await expect(page.getByRole("dialog", { name: "Eligibility checks" })).toBeVisible();
    await expectAccessible(page);
    await page.keyboard.press("Escape");
    await page.getByRole("button", { name: "Full reasoning" }).click();
    await expect(page.getByRole("dialog", { name: "Why Narrow surfaced it" })).toBeVisible();
    await expectAccessible(page);
  });
}

test("Settings can pin a theme over the system's", async ({ page }) => {
  await page.emulateMedia({ colorScheme: "dark" });
  await onboardViaApi("e2e-theme-pin");
  await signIn(page, "e2e-theme-pin", "/settings");
  await page.getByText("Light", { exact: true }).click();
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  // Rendered by the server on the next visit: no flash of the other theme.
  await page.goto("/today");
  await expect(page.locator("html")).toHaveAttribute("data-theme", "light");
  expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).toBe("rgb(247, 247, 248)");
  await page.goto("/settings");
  await page.getByText("System", { exact: true }).click();
  await expect(page.locator("html")).not.toHaveAttribute("data-theme");
});
