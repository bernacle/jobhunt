import axe from "axe-core";

/**
 * Automated accessibility checks on a rendered component. jsdom has no
 * layout, so rules that need it (color contrast) are left to the browser
 * tests; this is no substitute for a manual review.
 */
export async function violations(container: Element): Promise<string[]> {
  const results = await axe.run(container, {
    rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
  });
  return results.violations.map((v) => `${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(" ")).join(", ")})`);
}
