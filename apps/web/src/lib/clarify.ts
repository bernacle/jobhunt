import type { Clarify } from "./api-types";

/** "140,000 per year". */
export function amountText(c: Extract<Clarify, { kind: "pay" }>): string {
  return `${c.amount.toLocaleString("en-US")} per ${c.period}`;
}

/** What a question is about, in a few words (server and client alike). */
export function clarifyTitle(c: Clarify): string {
  switch (c.kind) {
    case "pay":
      return `Pay: ${amountText(c)}`;
    case "size":
      return c.value === "small_company" ? "Small companies" : "Small teams";
    case "importance":
      return capitalize(c.value);
  }
}

function capitalize(text: string): string {
  return text.charAt(0).toUpperCase() + text.slice(1);
}
