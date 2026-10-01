import type { RolesView } from "./api-types";

/** The one structured question of onboarding and Preferences (BRU-324). */
export const QUESTION = "What kind of role are you looking for?";

/** "Backend · Platform", or nothing. */
export function rolesText(roles: RolesView): string {
  return roles.chosen.map((r) => r.label).join(" · ");
}
