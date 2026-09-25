"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { CreatedToken, TokenView } from "@/lib/api-types";
import { ago } from "@/lib/format";

import { Button, Notice } from "./ui";

/**
 * Personal access tokens, for AI assistants that can't sign in with OAuth.
 * The secret is shown once, here, and never again.
 */
export function AssistantTokens({
  tokens,
  create,
  revoke,
}: {
  tokens: TokenView[];
  create: (name: string, days: number) => Promise<ActionResult<CreatedToken>>;
  revoke: (id: string) => Promise<ActionResult<void>>;
}) {
  const [created, setCreated] = useState<CreatedToken | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [pending, startTransition] = useTransition();
  const ids = { name: useId(), days: useId() };
  const active = tokens.filter((t) => !t.revoked_at && (!t.expires_at || new Date(t.expires_at) > new Date()));

  return (
    <div className="space-y-4">
      <form
        className="flex flex-col gap-2 sm:flex-row sm:items-end"
        onSubmit={(e) => {
          e.preventDefault();
          const data = new FormData(e.currentTarget);
          startTransition(async () => {
            setError(null);
            const r = await create(String(data.get("name") ?? ""), Number(data.get("days") ?? 30));
            if (r.ok) setCreated(r.data);
            else setError(`${r.title}. ${r.message}`);
          });
        }}
      >
        <div className="flex-1">
          <label htmlFor={ids.name} className="block text-sm font-medium">
            Token name
          </label>
          <input id={ids.name} name="name" defaultValue="Claude" maxLength={100} className="mt-1 w-full rounded-md border border-line-strong bg-surface px-3 py-2" />
        </div>
        <div>
          <label htmlFor={ids.days} className="block text-sm font-medium">
            Expires in
          </label>
          <select id={ids.days} name="days" defaultValue="30" className="mt-1 rounded-md border border-line-strong bg-surface px-2 py-2">
            <option value="7">7 days</option>
            <option value="30">30 days</option>
            <option value="90">90 days</option>
          </select>
        </div>
        <Button type="submit" disabled={pending}>
          Create token
        </Button>
      </form>
      {error && (
        <p role="alert" className="text-sm text-negative">
          {error}
        </p>
      )}
      {created && (
        <Notice tone="success" role="status" title="Copy it now: it won't be shown again">
          <code className="block break-all rounded bg-surface px-2 py-1 font-mono text-xs text-ink">{created.secret}</code>
        </Notice>
      )}
      {active.length > 0 && (
        <ul className="divide-y divide-line border-y border-line text-sm">
          {active.map((t) => (
            <li key={t.id} className="flex items-center justify-between gap-3 py-2">
              <span>
                {t.name}{" "}
                <span className="text-muted">
                  · created {ago(t.created_at)}
                  {t.last_used_at && ` · used ${ago(t.last_used_at)}`}
                </span>
              </span>
              <Button
                variant="quiet"
                disabled={pending}
                onClick={() => startTransition(async () => void (await revoke(t.id)))}
              >
                Revoke<span className="sr-only"> {t.name}</span>
              </Button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
