"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { NotificationSettingsView } from "@/lib/api-types";
import { ago } from "@/lib/format";

import { Button, Notice } from "./ui";

type Save = (request: {
  email_enabled?: boolean;
  cadence?: string;
  email?: string;
  resend_confirmation?: boolean;
}) => Promise<ActionResult<NotificationSettingsView>>;

/**
 * Email only when something is probably worth interrupting you for: new
 * strong matches, never "nothing found" and never a digest of maybes.
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
  const ids = { toggle: useId(), email: useId(), help: useId() };

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
    return <Notice title="Email isn't available on this JobHunt service yet.">Everything still shows up on Today.</Notice>;
  }

  const addressChanged = email.trim() !== (settings.email ?? "");
  return (
    <div className="space-y-5">
      <div className="flex items-start gap-3">
        <input
          id={ids.toggle}
          type="checkbox"
          className="mt-1 size-4 accent-[var(--accent)]"
          checked={settings.email_enabled}
          disabled={pending}
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
          <label htmlFor={ids.toggle} className="font-medium">
            Email me about strong new matches
          </label>
          <p className="text-sm text-muted">
            Only strong fits you haven&apos;t seen, verified at the employer, at most {settings.max_items} per email. No email
            when there&apos;s nothing worth it.
          </p>
        </div>
      </div>

      <fieldset className="space-y-1.5" disabled={pending}>
        <legend className="text-sm font-medium">How often, at most</legend>
        {[
          [
            "immediate",
            settings.min_interval_hours > 0
              ? `Soon after a match appears (at most one email every ${settings.min_interval_hours} hours)`
              : "Soon after a match appears",
          ],
          ["daily", "At most once a day"],
        ].map(([value, label]) => (
          <label key={value} className="flex items-center gap-2 text-sm">
            <input
              type="radio"
              name="cadence"
              value={value}
              checked={settings.cadence === value}
              onChange={() => apply({ cadence: value! }, "Saved.")}
              className="accent-[var(--accent)]"
            />
            {label}
          </label>
        ))}
      </fieldset>

      <form
        className="space-y-1.5"
        onSubmit={(e) => {
          e.preventDefault();
          if (email.trim()) apply({ email: email.trim() }, "We sent a confirmation link. Nothing else is sent until you follow it.");
        }}
      >
        <label htmlFor={ids.email} className="block text-sm font-medium">
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
            className="flex-1 rounded-md border border-line-strong bg-surface px-3 py-2"
          />
          <Button type="submit" disabled={pending || !addressChanged}>
            {settings.email ? "Change address" : "Use this address"}
          </Button>
        </div>
        <p id={ids.help} className="text-sm text-muted">
          {settings.email_status === "confirmed" && "Confirmed."}
          {settings.email_status === "unconfirmed" && (
            <>
              Waiting for you to follow the link we sent
              {settings.confirmation_sent_at && ` ${ago(settings.confirmation_sent_at)}`}.{" "}
              <button
                type="button"
                className="underline"
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

      <p aria-live="polite" className={`text-sm ${message?.tone === "error" ? "text-negative" : "text-muted"}`}>
        {message?.text}
      </p>

      {settings.recent.length > 0 && (
        <details className="text-sm">
          <summary className="cursor-pointer text-muted">Recent emails</summary>
          <ul className="mt-2 space-y-1">
            {settings.recent.map((d) => (
              <li key={d.id} className="text-muted">
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
