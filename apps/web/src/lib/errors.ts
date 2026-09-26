/**
 * The API's stable error codes, in words a person can act on. Raw server
 * messages (which can mention the CLI or internals) are never shown.
 */
export interface UiError {
  code: string;
  title: string;
  message: string;
}

const MESSAGES: Record<string, [string, string]> = {
  no_profile: ["Your profile isn't set up yet", "Upload a resume so Narrow knows what you've done."],
  unauthenticated: ["Your session has ended", "Sign in again to continue."],
  cloud_unavailable: [
    "We can't reach Narrow right now",
    "Decisions you made before this are already saved. Try again in a moment.",
  ],
  auth_unavailable: ["Sign-in is temporarily unavailable", "The identity provider didn't answer. Try again in a moment."],
  verification_unavailable: ["The listing couldn't be checked", "The employer's job board didn't answer. Narrow will try again later."],
  source_unavailable: ["Job boards couldn't be reached", "Stored jobs are still shown; Narrow keeps checking in the background."],
  conflict: ["Something changed at the same time", "Nothing was lost. Try that again."],
  invalid_preference: ["That preference couldn't be used", "Try saying it differently, or pick from the options."],
  invalid_arguments: ["That didn't work", "Check what you entered and try again."],
  invalid_request: ["That didn't work", "Check what you entered and try again."],
  unknown_opportunity: ["This opportunity is gone", "It may have been merged with another listing or removed."],
  ambiguous_id: ["That link is ambiguous", "Open the opportunity from Today or Applications instead."],
  email_unavailable: ["Email isn't available", "This Narrow service isn't set up to send email yet."],
  invalid_confirmation: ["That confirmation link didn't work", "It may have expired. Send a new one from Settings."],
  not_found: ["Not found", "That page or item doesn't exist."],
};

export function describeError(code: string | undefined): UiError {
  const key = code ?? "internal_error";
  const [title, message] = MESSAGES[key] ?? [
    "Something went wrong",
    "Narrow couldn't finish that. Try again; if it keeps happening, let us know.",
  ];
  return { code: key, title, message };
}
