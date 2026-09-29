import { expect, test } from "@playwright/test";

import { expectAccessible, onboardViaApi, signIn } from "./helpers";

/**
 * The eligibility checks open in the evidence panel (440px at the side, a
 * sheet on a phone). The rows follow the width their column really has, so
 * in the panel they stack at every viewport, each saying which part is the
 * posting's words; nothing is squeezed into an unreadable column and
 * nothing overflows.
 */
test("eligibility evidence stays readable at every width", async ({ page }) => {
  await onboardViaApi("e2e-layout");
  await signIn(page, "e2e-layout");
  const first = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
  await first.getByRole("heading", { level: 2 }).getByRole("link").click();
  await expect(page.getByRole("group", { name: "Eligibility", exact: true })).toBeVisible();

  for (const width of [1440, 1024, 390]) {
    await page.setViewportSize({ width, height: 900 });
    // The conclusion is on the page; the checks open over it.
    await page.getByRole("button", { name: "Checks for eligibility" }).click();
    const panel = page.getByRole("dialog", { name: "Eligibility checks" });
    await expect(panel).toBeVisible();
    const l = await panel.locator("[data-eligibility-row]").first().evaluate((row) => {
      const [rule, posting, verdict] = Array.from(row.children).map((c) => c.getBoundingClientRect());
      return {
        stacked: posting!.top >= rule!.bottom - 1 && verdict!.top >= posting!.bottom - 1,
        narrowest: Math.min(posting!.width, verdict!.width),
        overflows: row.scrollWidth > row.clientWidth + 1,
      };
    });
    expect(l.overflows, `${width}px`).toBe(false);
    expect(l.stacked, `${width}px`).toBe(true);
    expect(l.narrowest, `${width}px`).toBeGreaterThanOrEqual(250);
    await expect(panel.getByText("Posting says:").first()).toBeVisible();
    await page.keyboard.press("Escape");
    await expect(panel).toBeHidden();
  }
});

/**
 * Peers are compared side by side, so the same kind of information starts
 * in the same place in each (subgrid), whatever the lengths of the titles
 * or whether one has a concern; on a phone they stack in natural flow.
 */
test("peers share one geometry on wide screens and flow naturally on a phone", async ({ page }) => {
  await onboardViaApi("e2e-geometry");
  await signIn(page, "e2e-geometry");
  const peers = page.getByRole("list", { name: "Also worth a look" }).getByRole("article");
  await expect(peers.nth(1)).toBeVisible();
  const tops = (i: number) =>
    peers.nth(i).evaluate((card) => {
      const at = (el: Element | null) => (el ? Math.round(el.getBoundingClientRect().top) : null);
      return {
        card: at(card),
        facts: at(card.children[2]!),
        reason: at(card.children[3]!),
        verification: at(card.children[5]!),
        actions: at(card.children[6]!),
        bottom: Math.round(card.getBoundingClientRect().bottom),
      };
    });
  await page.setViewportSize({ width: 1440, height: 900 });
  const [a, b] = [await tops(0), await tops(1)];
  expect(b).toEqual(a);
  await page.setViewportSize({ width: 390, height: 900 });
  const [c, d] = [await tops(0), await tops(1)];
  // Stacked: the second starts where the first ends, with nothing reserved.
  expect(d.card).toBeGreaterThanOrEqual(c.bottom);
  const overflow = await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth);
  expect(overflow).toBeLessThanOrEqual(0);
});

test("an unusually long peer title and reason keep the grid aligned and remain readable on a phone", async ({ page }) => {
  await onboardViaApi("e2e-long-peer");
  await signIn(page, "e2e-long-peer");
  const peers = page.getByRole("list", { name: "Also worth a look" }).getByRole("article");
  await expect(peers.nth(1)).toBeVisible();
  const stressIndex = await peers.evaluateAll((cards) => cards.slice(0, 2).findIndex((card) => Boolean(card.children[4]?.querySelector("li"))));
  expect(stressIndex).toBeGreaterThanOrEqual(0);
  const stressed = peers.nth(stressIndex);

  const title = "Principal Staff Platform Engineer for Global Payments, Cross-Border Settlement, Identity, Risk, and Developer Infrastructure Across Multiple Regions";
  const reason = "This role matches your platform experience, but the description combines several teams and responsibilities, so you need to check the actual scope before deciding whether the day-to-day work fits what you want.";
  const condition = "Unresolved: you require remote roles open to Brazil, and this posting does not say whether its remote policy covers Brazil or whether regular office presence is required.";
  await stressed.evaluate((card, content) => {
    card.querySelector("h2 a")!.textContent = content.title;
    card.children[3]!.querySelector("p")!.textContent = content.reason;
    card.children[4]!.querySelector("li span:last-child")!.textContent = content.condition;
  }, { title, reason, condition });

  await page.setViewportSize({ width: 1440, height: 900 });
  const desktop = await peers.evaluateAll((cards) => cards.slice(0, 2).map((card) => ({
    facts: Math.round(card.children[2]!.getBoundingClientRect().top),
    reason: Math.round(card.children[3]!.getBoundingClientRect().top),
    concern: Math.round(card.children[4]!.getBoundingClientRect().top),
    verification: Math.round(card.children[5]!.getBoundingClientRect().top),
    actions: Math.round(card.children[6]!.getBoundingClientRect().top),
  })));
  expect(desktop[0]).toEqual(desktop[1]);
  const desktopFlow = await stressed.evaluate((card) => ({
    conditionBottom: card.children[4]!.getBoundingClientRect().bottom,
    verificationTop: card.children[5]!.getBoundingClientRect().top,
    actionsTop: card.children[6]!.getBoundingClientRect().top,
    actionsBottom: card.children[6]!.getBoundingClientRect().bottom,
    cardBottom: card.getBoundingClientRect().bottom,
  }));
  expect(desktopFlow.verificationTop).toBeGreaterThanOrEqual(desktopFlow.conditionBottom - 1);
  expect(desktopFlow.actionsTop).toBeGreaterThanOrEqual(desktopFlow.verificationTop);
  expect(desktopFlow.actionsBottom).toBeLessThanOrEqual(desktopFlow.cardBottom + 1);

  await page.setViewportSize({ width: 390, height: 900 });
  const mobile = await stressed.evaluate((card) => {
    const heading = card.querySelector("h2")!;
    const reason = card.children[3]!.querySelector("p")!;
    const concern = card.children[4]!.querySelector("li")!;
    const actions = card.children[6]!;
    const save = Array.from(actions.querySelectorAll("button")).find((button) => button.textContent?.trim() === "Save")!;
    return {
      titleClamped: getComputedStyle(heading).webkitLineClamp,
      reasonClamped: getComputedStyle(reason).webkitLineClamp,
      conditionClamped: getComputedStyle(concern).webkitLineClamp,
      titleHeight: heading.getBoundingClientRect().height,
      concernBelowReason: concern.getBoundingClientRect().top >= reason.getBoundingClientRect().bottom,
      actionsBelowConcern: actions.getBoundingClientRect().top >= concern.getBoundingClientRect().bottom,
      actionsWithinCard: actions.getBoundingClientRect().bottom <= card.getBoundingClientRect().bottom + 1,
      saveHeight: save.getBoundingClientRect().height,
      conditionText: concern.textContent,
    };
  });
  expect(mobile.titleClamped).toBe("none");
  expect(mobile.reasonClamped).toBe("none");
  expect(mobile.conditionClamped).toBe("none");
  expect(mobile.titleHeight).toBeGreaterThan(80);
  expect(mobile.concernBelowReason).toBe(true);
  expect(mobile.actionsBelowConcern).toBe(true);
  expect(mobile.actionsWithinCard).toBe(true);
  expect(mobile.saveHeight).toBeGreaterThanOrEqual(44);
  expect(mobile.conditionText).toContain(condition);
  const mobileCards = await peers.evaluateAll((cards) => cards.slice(0, 2).map((card) => card.getBoundingClientRect()));
  expect(mobileCards[1]!.top).toBeGreaterThanOrEqual(mobileCards[0]!.bottom);
  expect(await page.evaluate(() => document.documentElement.scrollWidth - document.documentElement.clientWidth)).toBeLessThanOrEqual(0);
  await expectAccessible(page);
  await stressed.getByRole("button", { name: "Save" }).click();
  await expect(stressed.getByRole("status")).toContainText("Saved");
});
