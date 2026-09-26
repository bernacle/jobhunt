"use client";

import { useId, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type { CreatedToken, TokenView } from "@/lib/api-types";
import { ago } from "@/lib/format";

import { Button, Notice, inlineActionClass, inputClass, labelClass, selectClass } from "./ui";

/**
 * Personal access tokens, for MCP clients that can't sign in with OAuth.
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
      <p className="text-[13px] font-medium text-fg">Access tokens</p>
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
        <div className="min-w-0 flex-1">
          <label htmlFor={ids.name} className={labelClass}>
            Token name
          </label>
          <input id={ids.name} name="name" defaultValue="Claude" maxLength={100} className={inputClass} />
        </div>
        <div>
          <label htmlFor={ids.days} className={labelClass}>
            Expires in
          </label>
          <select id={ids.days} name="days" defaultValue="30" className={`${selectClass} w-full sm:w-32`}>
            <option value="7">7 days</option>
            <option value="30">30 days</option>
            <option value="90">90 days</option>
          </select>
        </div>
        <Button type="submit" disabled={pending} loading={pending} className="h-9 max-sm:h-11">
          Create token
        </Button>
      </form>
      {error && (
        <p role="alert" className="text-[13px] text-danger">
          {error}
        </p>
      )}
      {created && (
        <Notice tone="success" role="status" title="Copy it now: it won't be shown again">
          <code className="mt-1 block rounded-md border border-line-subtle bg-inset px-2.5 py-2 font-mono text-mono-s break-all text-fg">
            {created.secret}
          </code>
        </Notice>
      )}
      {active.length > 0 && (
        <ul className="border-t border-line-subtle">
          {active.map((t) => (
            <li key={t.id} className="flex items-center justify-between gap-3 border-b border-line-subtle py-2.5">
              <span className="min-w-0 text-[14px]">
                {t.name}
                <span className="block font-mono text-mono-s text-fg-muted">
                  created {ago(t.created_at)}
                  {t.last_used_at && ` · used ${ago(t.last_used_at)}`}
                </span>
              </span>
              <button
                type="button"
                className={`${inlineActionClass} max-sm:min-h-11`}
                disabled={pending}
                onClick={() => startTransition(async () => void (await revoke(t.id)))}
              >
                Revoke<span className="sr-only"> {t.name}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
