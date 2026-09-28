"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { NotificationSettingsView } from "@/lib/api-types";
import { ago } from "@/lib/format";

import { Button, Notice, inlineActionClass, inputClass, labelClass } from "./ui";

type Save = (request: {
  email_enabled?: boolean;
  cadence?: string;
  email?: string;
  resend_confirmation?: boolean;
}) => Promise<ActionResult<NotificationSettingsView>>;

const choice =
  "size-4 shrink-0 cursor-pointer accent-[var(--nr-fg-primary)] disabled:cursor-default";

/**
 * Email only when something is probably worth interrupting you for: new
 * strong matches, never "nothing found" and never a digest of maybes. No
 * push alerts, no reminders to come back.
 */
export function NotificationSettings({
  initial,
  suggestedEmail,
  save,
}: {
  initial: NotificationSettingsView;
  suggestedEmail?: string;
  save: Save;
}) {
  const [settings, setSettings] = useState(initial);
  const [email, setEmail] = useState(initial.email ?? suggestedEmail ?? "");
  const [pending, startTransition] = useTransition();
  const [message, setMessage] = useState<{ tone: "error" | "success"; text: string } | null>(null);
  const ids = { toggle: useId(), toggleHelp: useId(), email: useId(), help: useId() };

  const apply = (request: Parameters<Save>[0], success?: string) => {
    // Toggles and choices show at once; the API's answer replaces them, and
    // a refusal puts them back.
    const before = settings;
    setSettings({
      ...settings,
      ...(request.email_enabled !== undefined ? { email_enabled: request.email_enabled } : {}),
      ...(request.cadence !== undefined ? { cadence: request.cadence } : {}),
    });
    startTransition(async () => {
      setMessage(null);
      const r = await save(request);
      if (r.ok) {
        setSettings(r.data);
        if (success) setMessage({ tone: "success", text: success });
      } else {
        setSettings(before);
        setMessage({ tone: "error", text: `${r.title}. ${r.message}` });
      }
    });
  };

  if (!settings.available) {
    return <Notice title="Email isn't available on this Narrow service yet.">Everything still shows up on Today.</Notice>;
  }

  const addressChanged = email.trim() !== (settings.email ?? "");
  return (
    <div>
      <div className="flex items-start gap-3 border-t border-line-subtle py-3.5">
        <input
          id={ids.toggle}
          type="checkbox"
          className={`${choice} mt-0.5`}
          checked={settings.email_enabled}
          disabled={pending}
          aria-describedby={ids.toggleHelp}
          onChange={(e) =>
            apply(
              e.target.checked && addressChanged && email.trim()
                ? { email_enabled: true, email: email.trim() }
                : { email_enabled: e.target.checked },
              e.target.checked ? "Saved. Emails are on." : "Saved. Emails are off.",
            )
          }
        />
        <div>
          <label htmlFor={ids.toggle} className="cursor-pointer text-[14px] font-medium">
            Email me about strong new matches
          </label>
          <p id={ids.toggleHelp} className="mt-0.5 text-[13px] leading-normal text-fg-secondary">
            Only verified strong fits you haven&apos;t seen, at most {settings.max_items} per email. Nothing when nothing is worth it.
          </p>
        </div>
      </div>

      <fieldset className="border-t border-line-subtle py-3.5" disabled={pending}>
        <legend className="float-left mb-2 w-full text-[13px] text-fg-muted">How often, at most</legend>
        <div className="clear-both space-y-1">
          {[
            [
              "immediate",
              settings.min_interval_hours > 0
                ? `Soon after a match appears (at most one email every ${settings.min_interval_hours} hours)`
                : "Soon after a match appears",
            ],
            ["daily", "At most once a day"],
          ].map(([value, label]) => (
            <label key={value} className="flex min-h-8 cursor-pointer items-center gap-3 text-[14px] max-sm:min-h-11">
              <input
                type="radio"
                name="cadence"
                value={value}
                checked={settings.cadence === value}
                onChange={() => apply({ cadence: value! }, "Saved.")}
                className={choice}
              />
              {label}
            </label>
          ))}
        </div>
      </fieldset>

      <form
        className="border-t border-line-subtle pt-3.5"
        onSubmit={(e) => {
          e.preventDefault();
          if (email.trim()) apply({ email: email.trim() }, "We sent a confirmation link. Nothing else is sent until you follow it.");
        }}
      >
        <label htmlFor={ids.email} className={labelClass}>
          Send to
        </label>
        <div className="flex flex-col gap-2 sm:flex-row">
          <input
            id={ids.email}
            type="email"
            autoComplete="email"
            required
            value={email}
            onChange={(e) => setEmail(e.target.value)}
            aria-describedby={ids.help}
            className={inputClass}
          />
          <Button type="submit" disabled={pending || !addressChanged} className="h-9 shrink-0 max-sm:h-11">
            {settings.email ? "Change address" : "Use this address"}
          </Button>
        </div>
        <p id={ids.help} className="mt-1.5 text-caption text-fg-muted">
          {settings.email_status === "confirmed" && "Confirmed."}
          {settings.email_status === "unconfirmed" && (
            <>
              Waiting for you to follow the link we sent
              {settings.confirmation_sent_at && ` ${ago(settings.confirmation_sent_at)}`}.{" "}
              <button
                type="button"
                className={`${inlineActionClass} text-[12px] underline underline-offset-2`}
                disabled={pending}
                onClick={() => apply({ resend_confirmation: true }, "Sent again.")}
              >
                Send it again
              </button>
            </>
          )}
          {settings.email_status === "none" && "We'll send a link to confirm the address first."}
        </p>
      </form>

      <p aria-live="polite" className={`mt-3 text-[13px] empty:mt-0 ${message?.tone === "error" ? "text-danger" : "text-fg-secondary"}`}>
        {message?.text}
      </p>

      {settings.recent.length > 0 && (
        <details className="group mt-3 text-[13px]">
          <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 text-fg-secondary hover:text-fg max-sm:min-h-11">
            <span aria-hidden="true" className="text-fg-muted transition-transform duration-[120ms] group-open:rotate-90">
              ›
            </span>
            Recent emails
          </summary>
          <ul className="mt-2 space-y-1 font-mono text-mono-s text-fg-muted">
            {settings.recent.map((d) => (
              <li key={d.id}>
                {d.kind === "confirmation" ? "Address confirmation" : `${d.opportunities} ${d.opportunities === 1 ? "match" : "matches"}`} ·{" "}
                {d.status} · {ago(d.sent_at ?? d.created_at)}
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
