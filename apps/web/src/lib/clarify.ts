import type { Clarify } from "./api-types";

/** "140,000 per year". */
export function amountText(c: Extract<Clarify, { kind: "pay" }>): string {
  return `${c.amount.toLocaleString("en-US")} per ${c.period}`;
}

/** What a question is about, in a few words (server and client alike). */
export function clarifyTitle(c: Clarify): string {
  return c.kind === "pay" ? `Pay: ${amountText(c)}` : c.value === "small_company" ? "Small companies" : "Small teams";
}
