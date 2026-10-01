"use client";

import { useId, useRef, useState, useTransition } from "react";

import type { RolesView, TasteProfileView, TasteUpdateResult } from "@/lib/api-types";
import { QUESTION, rolesText } from "@/lib/roles";

import { EditorForm } from "./preference-controls";
import { Sheet } from "./sheet";
import type { Review } from "./taste-profile";
import { Button, Notice, buttonClass, helpClass, inputClass, labelClass } from "./ui";

/*
 * "What kind of role are you looking for?" (BRU-324): the kinds of work the
 * person wants next, chosen from a short list, with an optional title in
 * their words for a role the list misses. Their career says what they have
 * done; this says what they want. One question, answerable in seconds: a
 * row of chips (native checkboxes, so Tab, Space and screen readers work
 * as everywhere), at most `max`.
 */

function labelOf(roles: RolesView, value: string): string {
  return roles.options.find((o) => o.value === value)?.label ?? value.replace(/_/g, " ");
}

/** The choices themselves: chips, a note on the limit, and the title. */
export function RolePicker({
  roles,
  chosen,
  onChosen,
  title,
  onTitle,
  disabled,
}: {
  roles: RolesView;
  chosen: string[];
  onChosen: (values: string[]) => void;
  title: string;
  onTitle: (title: string) => void;
  disabled?: boolean;
}) {
  const ids = { legend: useId(), limit: useId(), title: useId(), titleHelp: useId() };
  // What they chose before that the list doesn't offer (a specialty set
  // through the CLI) stays visible, and can be removed.
  const extra = roles.chosen.filter((c) => !roles.options.some((o) => o.value === c.value));
  const options = [...roles.options, ...extra];
  const full = chosen.length >= roles.max;
  const experience = roles.experience.map((v) => labelOf(roles, v));
  return (
    <div className="flex flex-col gap-5">
      <fieldset disabled={disabled} className="min-w-0" aria-describedby={ids.limit}>
        <legend id={ids.legend} className="mb-2 text-[14px] text-fg-secondary">
          Choose up to {roles.max}: the kind of engineering work you want next.
        </legend>
        <div className="flex flex-wrap gap-2">
          {options.map((o) => {
            const checked = chosen.includes(o.value);
            return (
              <label
                key={o.value}
                className={
                  "flex min-h-10 cursor-pointer items-center gap-2 rounded-md border px-3 text-[14px] transition-colors duration-[120ms] max-sm:min-h-11 " +
                  "has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) has-disabled:cursor-not-allowed has-disabled:opacity-50 " +
                  (checked ? "border-fg bg-selected font-medium text-fg" : "border-line text-fg-body hover:border-line-strong")
                }
              >
                <input
                  type="checkbox"
                  name="roles"
                  value={o.value}
                  checked={checked}
                  // Past the limit, only what is chosen can change.
                  disabled={!checked && full}
                  onChange={() => onChosen(checked ? chosen.filter((v) => v !== o.value) : [...chosen, o.value])}
                  className="size-4 accent-fg"
                />
                {o.label}
              </label>
            );
          })}
        </div>
        <p id={ids.limit} className={`${helpClass} nr-tnum`} aria-live="polite">
          {full ? `${chosen.length} of ${roles.max} chosen. Remove one to choose another.` : experience.length > 0 ? `Your experience shows ${experience.join(", ")}: that's what you've done; choose what you want next.` : null}
        </p>
      </fieldset>
      <div>
        <label htmlFor={ids.title} className={labelClass}>
          A title in your words <span className="text-fg-muted">(optional)</span>
        </label>
        <input
          id={ids.title}
          name="title"
          value={title}
          onChange={(e) => onTitle(e.target.value)}
          maxLength={roles.max_title}
          disabled={disabled}
          placeholder="Infrastructure-focused Product Engineer"
          autoComplete="off"
          aria-describedby={ids.titleHelp}
          className={inputClass}
        />
        <p id={ids.titleHelp} className={helpClass}>
          For a role the list misses. Kept as you write it; the kinds of work you choose are what Narrow looks for.
        </p>
      </div>
    </div>
  );
}

/** The choice as a draft: nothing is saved until Save, in one step. */
function useRoleDraft(roles: RolesView, review: Review, onSaved: (result: TasteUpdateResult) => void) {
  const [chosen, setChosen] = useState(() => roles.chosen.map((c) => c.value));
  const [title, setTitle] = useState(roles.title ?? "");
  const [error, setError] = useState<string | null>(null);
  const [pending, startTransition] = useTransition();
  const save = () => {
    if (chosen.length === 0 && !title.trim()) {
      setError("Choose at least one kind of role, or describe it in a few words.");
      return;
    }
    startTransition(async () => {
      setError(null);
      const r = await review({ action: "set_roles", roles: chosen, title: title.trim() || null });
      if (r.ok) onSaved(r.data);
      else setError(`${r.title}. ${r.message}`);
    });
  };
  // A change answers the error it was about.
  const choose = (values: string[]) => {
    setError(null);
    setChosen(values);
  };
  const retitle = (value: string) => {
    setError(null);
    setTitle(value);
  };
  return { chosen, setChosen: choose, title, setTitle: retitle, error, pending, save };
}

/** The question on a page (onboarding): the choices and a Continue. */
export function RolesForm({ roles, review, submitLabel = "Continue", onSaved }: { roles: RolesView; review: Review; submitLabel?: string; onSaved?: (result: TasteUpdateResult) => void }) {
  const draft = useRoleDraft(roles, review, (r) => onSaved?.(r));
  return (
    <form
      noValidate
      aria-label={QUESTION}
      onSubmit={(e) => {
        e.preventDefault();
        draft.save();
      }}
    >
      <RolePicker roles={roles} chosen={draft.chosen} onChosen={draft.setChosen} title={draft.title} onTitle={draft.setTitle} disabled={draft.pending} />
      <div className="mt-4 flex flex-wrap items-center gap-3">
        <Button type="submit" variant="primary" disabled={draft.pending} loading={draft.pending} className="max-sm:h-11">
          {submitLabel}
        </Button>
      </div>
      {draft.error && (
        <div className="mt-3">
          <Notice tone="error" role="alert" title="Not saved">
            {draft.error}
          </Notice>
        </div>
      )}
    </form>
  );
}

/** The focused editor in a sheet: Cancel leaves everything as it was. */
function RolesEditor({ roles, review, done }: { roles: RolesView; review: Review; done: (result?: TasteUpdateResult) => void }) {
  const draft = useRoleDraft(roles, review, done);
  return (
    <EditorForm onSubmit={draft.save} onCancel={() => done()} pending={draft.pending} error={draft.error}>
      <RolePicker roles={roles} chosen={draft.chosen} onChosen={draft.setChosen} title={draft.title} onTitle={draft.setTitle} disabled={draft.pending} />
    </EditorForm>
  );
}

/**
 * "What you're looking for" on Preferences: the kinds of role chosen, with
 * Change; until they answer, the question itself, compact, above the rest
 * (nothing else on the page waits for it).
 */
export function TargetRoles({ profile: initial, review }: { profile: TasteProfileView; review: Review }) {
  const [latest, setLatest] = useState<{ from: TasteProfileView; roles: RolesView } | null>(null);
  const roles = latest && latest.from === initial ? latest.roles : initial.roles;
  const [editing, setEditing] = useState(false);
  const [status, setStatus] = useState("");
  const trigger = useRef<HTMLButtonElement>(null);
  const close = (result?: TasteUpdateResult) => {
    if (result) {
      setLatest({ from: initial, roles: result.profile.roles });
      setStatus(result.changed ? "Saved." : "Already in effect.");
    }
    setEditing(false);
    requestAnimationFrame(() => trigger.current?.focus());
  };
  const text = rolesText(roles);
  return (
    <section id="roles" aria-labelledby="roles-heading" className="scroll-mt-20">
      <div className="mb-1.5 flex items-baseline justify-between gap-4">
        <h2 id="roles-heading" className="text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
          {roles.answered ? "What you're looking for" : QUESTION}
        </h2>
        {roles.answered && (
          <button
            ref={trigger}
            type="button"
            aria-haspopup="dialog"
            aria-expanded={editing}
            onClick={() => {
              setStatus("");
              setEditing(true);
            }}
            className="cursor-pointer text-[13px] font-medium text-accent hover:text-accent-hover max-sm:min-h-11"
          >
            Change <span className="sr-only">the kinds of role you&apos;re looking for</span>
          </button>
        )}
      </div>
      <div className="border-t border-line-subtle pt-3">
        {roles.answered ? (
          <>
            {text && <p className="text-[15px] leading-[1.5] font-medium text-pretty text-fg">{text}</p>}
            {roles.title && <q className="mt-0.5 block text-[14px] break-words text-fg-body">{roles.title}</q>}
          </>
        ) : (
          <div className="flex flex-col gap-3">
            <p className="text-[14px] text-pretty text-fg-body">
              Pick the kind of engineering work you want next. Your career shows what you&apos;ve done; this tells Narrow what to look for.
            </p>
            <div>
              <button ref={trigger} type="button" aria-haspopup="dialog" aria-expanded={editing} onClick={() => setEditing(true)} className={buttonClass("primary", "md", "max-sm:h-11")}>
                Choose roles
              </button>
            </div>
          </div>
        )}
        <p role="status" className="mt-1 text-caption text-fg-muted empty:hidden">
          {status}
        </p>
      </div>
      <Sheet
        open={editing}
        onClose={() => close()}
        title={QUESTION}
        subtitle={roles.answered ? `Now: ${[text, roles.title].filter(Boolean).join(" · ")}` : "Not answered yet"}
        fit
        closeWord={false}
        initialFocus={(panel) => panel.querySelector<HTMLElement>("input[name=roles]:checked") ?? panel.querySelector<HTMLElement>("input[name=roles]")}
      >
        {editing && <RolesEditor roles={roles} review={review} done={close} />}
      </Sheet>
    </section>
  );
}
