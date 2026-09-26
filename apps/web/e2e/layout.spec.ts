import { expect, test } from "@playwright/test";

import { onboardViaApi, signIn } from "./helpers";

/**
 * The eligibility rows follow the width their column really has. Beside
 * the aside at 1024px that column is narrow, so the rows stack; on a wide
 * desktop they are three columns; on a phone they stack again. Either way
 * nothing is squeezed into an unreadable column.
 */
test("eligibility evidence stays readable at every width", async ({ page }) => {
  await onboardViaApi("e2e-layout");
  await signIn(page, "e2e-layout");
  const first = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  await first.getByRole("heading", { level: 2 }).getByRole("link").click();
  const eligibility = page.getByRole("region", { name: "Eligibility" });
  await expect(eligibility).toBeVisible();

  const layout = () =>
    eligibility.locator("[data-eligibility-row]").first().evaluate((row) => {
      const [rule, posting, verdict] = Array.from(row.children).map((c) => c.getBoundingClientRect());
      return {
        sideBySide: Math.abs(posting!.top - verdict!.top) < 4 && verdict!.left > posting!.right,
        stacked: posting!.top >= rule!.bottom - 1 && verdict!.top >= posting!.bottom - 1,
        narrowest: Math.min(posting!.width, verdict!.width),
        overflows: row.scrollWidth > row.clientWidth + 1,
      };
    });

  for (const [width, expected] of [
    [1440, "columns"],
    [1024, "stacked"],
    [390, "stacked"],
  ] as const) {
    await page.setViewportSize({ width, height: 900 });
    await expect(eligibility).toBeVisible();
    const l = await layout();
    expect(l.overflows, `${width}px`).toBe(false);
    if (expected === "columns") {
      expect(l.sideBySide, `${width}px`).toBe(true);
      expect(l.narrowest, `${width}px`).toBeGreaterThanOrEqual(150);
    } else {
      expect(l.stacked, `${width}px`).toBe(true);
      expect(l.narrowest, `${width}px`).toBeGreaterThanOrEqual(250);
      await expect(eligibility.getByText("Posting says:").first()).toBeVisible();
    }
  }
});
