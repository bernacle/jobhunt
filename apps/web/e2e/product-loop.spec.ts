import { expect, test } from "@playwright/test";

import { RESUME, STATEMENT, expectAccessible, publishLaterJobs, runWorker, sentEmails, signIn } from "./helpers";

/**
 * The main loop, as a person does it: sign in, upload a resume, say what
 * they want, open Today, review, reject one with a reason, save another,
 * mark one applied, check Applications and Preferences, get an email about
 * a strong match they haven't seen, sign out and back in, and find
 * everything as they left it.
 */
test.describe.serial("the product loop", () => {
  const name = "e2e-ana";
  let rejected = "";
  let saved = "";
  let applied = "";

  test("sign in and onboard", async ({ page }) => {
    await signIn(page, name);
    await expect(page).toHaveURL(/\/welcome$/);
    await page.getByLabel("Your resume", { exact: true }).setInputFiles(RESUME);
    await page.getByRole("button", { name: "Import" }).click();
    await expect(page.getByText("Resume imported")).toBeVisible();

    await page.getByLabel("What are you looking for?").fill(STATEMENT);
    await page.getByRole("button", { name: "Update preferences" }).click();
    await expect(page.getByRole("heading", { name: "Understood" })).toBeVisible();
    await expect(page.getByText("backend roles").first()).toBeVisible();
    await expect(page.getByRole("heading", { name: "I couldn't interpret" })).toBeVisible();
    await expect(page.getByText("Something about good vibes")).toBeVisible();
    await page.getByRole("link", { name: "Go to Today" }).click();
    await expect(page).toHaveURL(/\/today$/);
  });

  test("Today is a short list of verified, explained recommendations", async ({ page }) => {
    await signIn(page, name);
    const cards = page.getByRole("list", { name: "Recommendations" }).getByRole("article");
    await expect(cards.first()).toBeVisible();
    const count = await cards.count();
    expect(count).toBeGreaterThanOrEqual(3);
    expect(count).toBeLessThanOrEqual(5);
    await expect(page.getByText(/Checked \d+ open jobs/)).toBeVisible();
    // Only strong fits and jobs worth reviewing: no maybes, never the
    // unwanted SRE role or the marketing job.
    await expect(page.getByText("Maybe", { exact: true })).toHaveCount(0);
    await expect(page.getByText("Site Reliability Engineer")).toHaveCount(0);
    await expect(page.getByText("Marketing Manager")).toHaveCount(0);
    // One raised lead with visible section labels; denser peers after it.
    await expect(cards.first().getByText("Why it may be worth your time")).toBeVisible();
    await expect(cards.first().getByText("Things to consider")).toBeVisible();
    await expect(page.getByText("Also worth a look")).toBeVisible();
    // Why, what to consider, pay and verification on every card.
    for (let i = 0; i < count; i++) {
      const card = cards.nth(i);
      await expect(card.getByText("Why it may be worth your time")).toHaveCount(1);
      await expect(card.getByText("Things to consider")).toHaveCount(1);
      await expect(card.getByText(/USD [0-9,]+ – [0-9,]+ per year/)).toBeVisible();
      await expect(card.getByText(/Verified .* on the employer's job board/)).toBeVisible();
      await expect(card.getByRole("button")).toHaveText(["Not now", "Not for me", "I applied", "Save"]);
    }
    // Tiers are words, never scores.
    await expect(page.locator("main")).not.toContainText(/\d+\s?%/);
    // No infinite scroll: the page ends.
    await expect(page.getByText("That's everything new worth your time.")).toBeVisible();
    await expectAccessible(page);

    // Reloading keeps the same recommendations.
    const titles = await cards.getByRole("heading", { level: 2 }).allTextContents();
    await page.reload();
    await expect(cards.getByRole("heading", { level: 2 })).toHaveText(titles);
  });

  test("the full brief explains one opportunity", async ({ page }) => {
    await signIn(page, name);
    const first = page.getByRole("list", { name: "Recommendations" }).getByRole("article").first();
    const title = (await first.getByRole("heading", { level: 2 }).textContent()) ?? "";
    await first.getByRole("link", { name: title, exact: true }).click();
    await expect(page.getByRole("heading", { level: 1, name: title })).toBeVisible();
    await expect(page.getByText("Decision brief")).toBeVisible();
    for (const section of ["Eligibility", "Compensation", "Location and employment", "Full description", "Verification", "Provenance"]) {
      await expect(page.getByRole("heading", { level: 2, name: section })).toBeVisible();
    }
    await expect(page.getByText(/A compatibility signal/)).toBeVisible();
    await expect(page.getByRole("link", { name: /^Greenhouse · / })).toBeVisible();
    await expect(page.getByRole("navigation", { name: "Breadcrumb" }).getByRole("link", { name: "Today" })).toBeVisible();
    await expectAccessible(page);
  });

  test("reject with a reason, save, mark applied", async ({ page }) => {
    await signIn(page, name);
    const cards = page.getByRole("list", { name: "Recommendations" }).getByRole("article");
    await expect(cards.nth(2)).toBeVisible();
    const titles = await cards.getByRole("heading", { level: 2 }).allTextContents();
    [rejected, saved, applied] = titles as [string, string, string];

    // Keyboard only, for the rejection.
    const firstCard = cards.nth(0);
    await firstCard.getByRole("button", { name: "Not for me" }).focus();
    await page.keyboard.press("Enter");
    const dialog = page.getByRole("dialog", { name: "Not for me" });
    await expect(dialog).toBeVisible();
    await expect(dialog.getByLabel(/What didn't fit/)).toBeFocused();
    await page.keyboard.press("Escape");
    await expect(dialog).toBeHidden();
    await firstCard.getByRole("button", { name: "Not for me" }).click();
    await dialog.getByLabel(/What didn't fit/).fill("On-call heavy");
    await dialog.getByRole("button", { name: "Too corporate" }).click();
    await dialog.getByRole("button", { name: "Mark not for me" }).click();
    await expect(cards.nth(0).getByRole("status")).toContainText("Won't be recommended again");

    await cards.nth(1).getByRole("button", { name: "Save" }).click();
    await expect(cards.nth(1).getByRole("status")).toContainText("Saved");
    await cards.nth(2).getByRole("button", { name: "I applied" }).click();
    await expect(cards.nth(2).getByRole("status")).toContainText("Marked as applied");

    // A fresh Today no longer shows any of the three.
    await page.reload();
    await expect(page.getByRole("list", { name: "Recommendations" }).getByRole("article").first()).toBeVisible();
    const remaining = await page.getByRole("list", { name: "Recommendations" }).getByRole("heading", { level: 2 }).allTextContents();
    for (const title of [rejected, saved, applied]) expect(remaining).not.toContain(title);
  });

  test("Applications tracks the pipeline", async ({ page }) => {
    await signIn(page, name, "/applications");
    const appliedSection = page.getByRole("region", { name: /Applied/ });
    await expect(appliedSection.getByRole("link", { name: applied, exact: true })).toBeVisible();
    await expect(page.getByRole("region", { name: /Saved/ }).getByRole("link", { name: saved, exact: true })).toBeVisible();
    await expect(page.getByRole("link", { name: rejected, exact: true })).toBeHidden();
    await appliedSection.getByRole("button", { name: new RegExp(`^Interviewing — ${applied.replace(/[()]/g, "\\$&")}`) }).click();
    await expect(page.getByRole("region", { name: /Interview/ }).getByRole("link", { name: applied, exact: true })).toBeVisible();
    // The stage tabs narrow the list to one stage.
    await page.getByRole("tab", { name: /^Saved/ }).click();
    await expect(page.getByRole("tabpanel").getByRole("link", { name: saved, exact: true })).toBeVisible();
    await expect(page.getByRole("tabpanel").getByRole("link", { name: applied, exact: true })).toBeHidden();
    await page.getByRole("tab", { name: /^All/ }).click();
    await page.getByText(/Not for me \(1\)/).click();
    await expect(page.getByRole("link", { name: rejected, exact: true })).toBeVisible();
    await expect(page.getByText("On-call heavy; too corporate")).toBeVisible();
    await expectAccessible(page);
  });

  test("Preferences keep what was said apart from what was learned", async ({ page }) => {
    await signIn(page, name, "/preferences");
    await expect(page.getByRole("columnheader", { name: /You told us/ })).toBeVisible();
    await expect(page.getByRole("columnheader", { name: /We've learned.*Ranking only/ })).toBeVisible();
    const roles = page.getByRole("row", { name: /^Roles/ });
    await expect(roles.getByRole("cell").first().getByText("Want: backend roles")).toBeVisible();
    const pay = page.getByRole("row", { name: /^Pay/ });
    await expect(pay.getByRole("cell").first().getByText("Must: at least USD 120,000 per year")).toBeVisible();
    const company = page.getByRole("row", { name: /^Company and team/ });
    await expect(company.getByRole("cell").nth(1).getByText(/You tend to pass on large companies/)).toBeVisible();
    // A structured preference, the same model the API and assistants use.
    await page.getByLabel("About").selectOption("domain");
    await page.getByLabel("Rule").selectOption("avoid");
    await page.getByLabel("Value").fill("adtech");
    await page.getByRole("button", { name: "Add preference" }).click();
    await expect(page.getByText("Saved.")).toBeVisible();
    await expect(page.getByRole("row", { name: /^Domains and products/ }).getByText("Avoid: adtech")).toBeVisible();
    await expect(page.getByRole("region", { name: "In your words" }).getByText(STATEMENT)).toBeVisible();
    await expectAccessible(page);
  });

  test("Profile shows the model and takes claim decisions", async ({ page }) => {
    await signIn(page, name, "/profile");
    await expect(page.getByRole("region", { name: "Sources" }).getByText(/^ana_lima\.md\s*current$/)).toBeVisible();
    await expect(page.getByRole("heading", { name: "Experience" })).toBeVisible();
    const review = page.getByRole("region", { name: "Needs your review" });
    const before = await review.getByText(/claims? needs? your review/).textContent();
    await review.getByRole("button", { name: /^Confirm/ }).first().click();
    await expect(review.getByText(/Confirmed: it can be used as evidence/)).toBeVisible();
    expect(await review.getByText(/claims? needs? your review|Nothing left/).textContent()).not.toBe(before);
    await expectAccessible(page);
  });

  test("an email about a strong match not seen yet, once", async ({ page }) => {
    await signIn(page, name, "/settings");
    await page.getByLabel("Send to").fill("e2e-ana@example.com");
    await page.getByRole("button", { name: "Use this address" }).click();
    await expect(page.getByText(/sent a confirmation link/)).toBeVisible();
    const confirmation = sentEmails().find((m) => m.message.to === "e2e-ana@example.com");
    expect(confirmation?.message.subject).toBe("Confirm your email for Narrow notifications");
    const link = /http:\/\/127\.0\.0\.1:3100\/settings\/confirm\?token=\w+/.exec(confirmation!.message.text)![0];
    await page.goto(link);
    await expect(page.getByRole("heading", { name: "Email confirmed" })).toBeVisible();
    await page.goto("/settings");
    await page.getByLabel("Email me about strong new matches").check();
    await expect(page.getByText("Saved. Emails are on.")).toBeVisible();
    // Saved on the server, not just on screen.
    await page.reload();
    await expect(page.getByLabel("Email me about strong new matches")).toBeChecked();

    // Everything strong was already seen on Today: nothing to email.
    const quiet = await runWorker("notify");
    expect(quiet.sent, JSON.stringify(quiet)).toBe(0);
    expect(quiet.nothing_new).toBe(1);

    // A new opening appears; the scheduled workers find and verify it.
    await publishLaterJobs("latecomer");
    const discovered = await runWorker("discovery");
    expect(discovered.new).toBe(1);
    await runWorker("verification");
    const first = await runWorker("notify");
    expect(first.sent, JSON.stringify(first)).toBe(1);
    const emails = sentEmails().filter((m) => m.message.to === "e2e-ana@example.com" && !m.message.subject.startsWith("Confirm"));
    expect(emails).toHaveLength(1);
    const email = emails[0]!.message;
    // A strong fit the person hasn't seen on Today; nothing already dealt with.
    expect(email.subject).toBe("A strong new match: Staff Backend Engineer, Payments Platform at Latecomer");
    for (const title of [rejected, saved, applied]) expect(email.text).not.toContain(title);
    expect(email.text).toContain("http://127.0.0.1:3100/opportunities/opp_");
    // Nothing new: no email.
    const second = await runWorker("notify");
    expect(second.sent).toBe(0);
    expect(sentEmails().filter((m) => m.message.to === "e2e-ana@example.com")).toHaveLength(2);
  });

  test("signing out and back in keeps everything", async ({ page }) => {
    await signIn(page, name, "/settings");
    await page.getByRole("button", { name: "Sign out", exact: true }).first().click();
    await expect(page).toHaveURL(/\/signin\?signed_out=1/);
    await page.goto("/today");
    await expect(page).toHaveURL(/\/signin\?next=%2Ftoday/);
    await signIn(page, name);
    await expect(page.getByRole("heading", { level: 1, name: "Today" })).toBeVisible();
    // Past the loading skeleton: the feed itself is there.
    await expect(page.getByRole("list", { name: "Recommendations" }).getByRole("article").first()).toBeVisible();
    const titles = await page.getByRole("list", { name: "Recommendations" }).getByRole("heading", { level: 2 }).allTextContents();
    for (const title of [rejected, saved, applied]) expect(titles).not.toContain(title);
    await page.goto("/applications");
    await expect(page.getByRole("region", { name: /Interview/ }).getByRole("link", { name: applied, exact: true })).toBeVisible();
    await expect(page.getByRole("region", { name: /Saved/ }).getByRole("link", { name: saved, exact: true })).toBeVisible();
  });

  test("dealing with everything leaves you caught up", async ({ page }) => {
    await signIn(page, name);
    const caughtUp = page.getByRole("heading", { name: "You're caught up." });
    const list = page.getByRole("list", { name: "Recommendations" });
    for (let round = 0; round < 5; round++) {
      // The loading skeleton has the "Today" heading too: wait for the feed
      // itself (a list, or caught up) before counting what's on it.
      await expect(list.or(caughtUp)).toBeVisible();
      if (await caughtUp.isVisible()) break;
      const cards = list.getByRole("article");
      const count = await cards.count();
      for (let i = 0; i < count; i++) {
        await cards.nth(i).getByRole("button", { name: "Not now" }).click();
        await expect(cards.nth(i).getByRole("status")).toContainText("Put aside");
      }
      await page.reload();
    }
    await expect(page.getByRole("heading", { name: "You're caught up." })).toBeVisible();
    await expect(page.getByText(/0 jobs/)).toHaveCount(0);
    await expect(page.getByRole("link", { name: /1 saved/ })).toBeVisible();
    await expectAccessible(page);
  });
});
