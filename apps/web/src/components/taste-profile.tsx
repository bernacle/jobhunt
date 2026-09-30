"use client";

import { type ReactNode, useActionState, useId, useRef, useState, useTransition } from "react";

import type { ActionResult, DescribeState } from "@/app/actions";
import type { ConstraintView, PolarityInput, TasteAction, TasteItemView, TasteLineView, TasteProfileView, TasteUpdateResult } from "@/lib/api-types";

import { LayerTag } from "./preference-controls";
import { Sheet, SheetBody } from "./sheet";
import { Disclosure } from "./summary";
import { Button, Notice, buttonClass, helpClass, inlineActionClass, inputClass, labelClass, selectClass, textareaClass } from "./ui";

/*
 * Preferences as what Narrow understands about the person, not as ranking
 * settings: their words, a short summary of the kind of role and company
 * they want (and avoid), and their practical constraints, kept apart.
 * Where each statement comes from is one tap away, never in the way; what
 * Narrow read or inferred is never presented as something they said.
 */

export type Describe = (state: DescribeState, form: FormData) => Promise<DescribeState>;
export type Review = (action: TasteAction) => Promise<ActionResult<TasteUpdateResult>>;

const EXAMPLE = "Small technical teams, high ownership, backend or platform work at startups. No early-career roles or process-heavy big companies.";

/** The one question, in a form. */
export function DescribeForm({
  describe,
  initial,
  submitLabel = "Save",
  onCancel,
  onDone,
}: {
  describe: Describe;
  initial?: string;
  submitLabel?: string;
  onCancel?: () => void;
  onDone?: (result: TasteUpdateResult) => void;
}) {
  const [state, action, pending] = useActionState(async (previous: DescribeState, form: FormData) => {
    const next = await describe(previous, form);
    if (next.result) onDone?.(next.result);
    return next;
  }, {});
  const id = useId();
  const help = useId();
  return (
    <form action={action}>
      <label htmlFor={id} className={labelClass}>
        What kind of job are you looking for?
      </label>
      <textarea
        id={id}
        name="text"
        rows={3}
        required
        maxLength={2000}
        aria-describedby={help}
        defaultValue={state.error ? state.submitted : initial}
        placeholder={EXAMPLE}
        className={textareaClass}
      />
      <p id={help} className={helpClass}>
        In your own words; what you&apos;d avoid, too. Narrow keeps them as written.
      </p>
      <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2">
        <Button type="submit" variant="primary" disabled={pending} loading={pending} className="max-sm:h-11">
          {pending ? "Reading…" : submitLabel}
        </Button>
        {onCancel && (
          <Button variant="ghost" onClick={onCancel} disabled={pending} className="max-sm:h-11">
            Cancel
          </Button>
        )}
      </div>
      {state.error && (
        <div aria-live="polite" className="mt-3">
          <Notice tone="error" role="alert" title={state.error.title}>
            {state.error.message}
          </Notice>
        </div>
      )}
    </form>
  );
}

/** "What you're looking for": their words, and a way to change them. */
export function LookingFor({ profile, describe, review }: { profile: TasteProfileView; describe: Describe; review: Review }) {
  const [editing, setEditing] = useState(!profile.looking_for);
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const earlier = profile.looking_for_source === "statements";
  return (
    <section id="looking-for" aria-labelledby="looking-for-heading" className="scroll-mt-20">
      <div className="mb-1.5 flex items-baseline justify-between gap-4">
        <h2 id="looking-for-heading" className="text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
          What you&apos;re looking for
        </h2>
        {!editing && !earlier && (
          <button type="button" onClick={() => setEditing(true)} className={`${inlineActionClass} max-sm:min-h-11`}>
            Edit <span className="sr-only">what you&apos;re looking for</span>
          </button>
        )}
      </div>
      <div className="border-t border-line-subtle pt-3">
        {editing ? (
          <DescribeForm
            describe={describe}
            initial={earlier ? undefined : (profile.looking_for ?? undefined)}
            onCancel={profile.looking_for && !earlier ? () => setEditing(false) : undefined}
            onDone={() => setEditing(false)}
          />
        ) : earlier ? (
          <div>
            <p className="text-caption text-fg-muted">What you told Narrow before:</p>
            <q className="mt-1 block text-[15px] leading-[1.55] whitespace-pre-line text-pretty text-fg-body">{profile.looking_for}</q>
            <div className="mt-3 flex flex-wrap gap-x-3 gap-y-2">
              <Button
                variant="secondary"
                disabled={pending}
                loading={pending}
                className="max-sm:h-11"
                onClick={() =>
                  startTransition(async () => {
                    setError(null);
                    const r = await review({ action: "reinterpret" });
                    if (!r.ok) setError(`${r.title}. ${r.message}`);
                  })
                }
              >
                Use these words
              </Button>
              <Button variant="ghost" onClick={() => setEditing(true)} disabled={pending} className="max-sm:h-11">
                Describe it again
              </Button>
            </div>
            {error && (
              <p role="alert" className="mt-2 text-[13px] text-danger">
                {error}
              </p>
            )}
          </div>
        ) : (
          <q className="block text-[15px] leading-[1.55] text-pretty text-fg-body">{profile.looking_for}</q>
        )}
      </div>
    </section>
  );
}

function Line({ line }: { line: TasteLineView }) {
  return (
    <li className="flex gap-2.5 py-1">
      <span aria-hidden="true" className="mt-[0.6em] size-[5px] shrink-0 rounded-[1px] bg-fg" />
      <span className="min-w-0 text-[15px] leading-[1.5] text-pretty text-fg-body">
        {line.text}
        {line.inferred && <span className="text-[13px] text-fg-muted"> · from your profile</span>}
      </span>
    </li>
  );
}

/** Where a statement comes from, for the details. */
function Provenance({ item }: { item: TasteItemView }) {
  return (
    <li className="border-t border-line-subtle py-2.5">
      <p className="text-[14px] text-fg">
        {item.polarity === "avoid" ? "Avoid: " : ""}
        {item.text}
      </p>
      <p className="mt-0.5 text-caption text-fg-muted">
        {item.basis}
        {item.confidence !== "high" && item.origin !== "stated" && ` · ${item.confidence} confidence`}
      </p>
      {item.sources
        .filter((s) => s.kind !== "person")
        .map((s, i) => (
          <p key={i} className="text-caption text-fg-muted">
            {s.text}
          </p>
        ))}
      {item.original && <p className="text-caption text-fg-muted">Narrow had read: {item.original}</p>}
      {(item.against ?? []).map((s, i) => (
        <p key={`a${i}`} className="text-caption text-fg-muted">
          Points the other way: {s.text}
        </p>
      ))}
    </li>
  );
}

const POLARITIES: { value: PolarityInput; label: string }[] = [
  { value: "prefer", label: "Want" },
  { value: "open", label: "Open to" },
  { value: "avoid", label: "Avoid" },
  { value: "neutral", label: "Doesn't matter" },
];

/** One statement in the correction sheet: change it, say it doesn't matter, or remove it. */
function Correctable({ item, run, busy }: { item: TasteItemView; run: (action: TasteAction) => Promise<boolean>; busy: boolean }) {
  const [changing, setChanging] = useState(false);
  const [text, setText] = useState(item.text);
  const [polarity, setPolarity] = useState<PolarityInput>(item.polarity as PolarityInput);
  const ids = { text: useId(), polarity: useId() };
  return (
    <li className="border-t border-line-subtle py-3">
      <p className="text-[14px] font-medium text-fg">
        {item.polarity === "avoid" && <span className="text-fg-secondary">Avoid: </span>}
        {item.text}
      </p>
      <p className="text-caption text-fg-muted">{item.basis}</p>
      {changing ? (
        <form
          className="mt-2 grid gap-2.5"
          onSubmit={async (e) => {
            e.preventDefault();
            const changed = text.trim() !== item.text;
            const ok = await run({
              action: "correct",
              id: item.id,
              text: changed ? text.trim() : null,
              polarity: polarity !== item.polarity ? polarity : null,
            });
            if (ok) setChanging(false);
          }}
        >
          <div>
            <label htmlFor={ids.text} className={labelClass}>
              In your words
            </label>
            <input id={ids.text} value={text} onChange={(e) => setText(e.target.value)} className={inputClass} autoComplete="off" />
          </div>
          <div>
            <label htmlFor={ids.polarity} className={labelClass}>
              How you feel about it
            </label>
            <select id={ids.polarity} value={polarity} onChange={(e) => setPolarity(e.target.value as PolarityInput)} className={selectClass}>
              {POLARITIES.map((p) => (
                <option key={p.value} value={p.value}>
                  {p.label}
                </option>
              ))}
            </select>
          </div>
          <div className="flex gap-2">
            <Button type="submit" variant="primary" disabled={busy} loading={busy} className="max-sm:h-11">
              Save
            </Button>
            <Button variant="ghost" onClick={() => setChanging(false)} disabled={busy} className="max-sm:h-11">
              Cancel
            </Button>
          </div>
        </form>
      ) : (
        <div className="mt-1.5 flex flex-wrap gap-x-4">
          <button type="button" disabled={busy} onClick={() => setChanging(true)} className={`${inlineActionClass} max-sm:min-h-11`}>
            Change <span className="sr-only">{item.text}</span>
          </button>
          {item.polarity !== "neutral" && (
            <button type="button" disabled={busy} onClick={() => run({ action: "neutral", id: item.id })} className={`${inlineActionClass} max-sm:min-h-11`}>
              Doesn&apos;t matter <span className="sr-only">({item.text})</span>
            </button>
          )}
          <button type="button" disabled={busy} onClick={() => run({ action: "remove", id: item.id })} className={`${inlineActionClass} max-sm:min-h-11`}>
            Remove <span className="sr-only">{item.text}</span>
          </button>
        </div>
      )}
    </li>
  );
}

/** The focused correction surface: every statement of the summary, and one more sentence. */
function Corrections({ profile, review, onUpdated }: { profile: TasteProfileView; review: Review; onUpdated: (p: TasteProfileView) => void }) {
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const [sentence, setSentence] = useState("");
  const addId = useId();
  const run = (action: TasteAction) =>
    new Promise<boolean>((resolve) =>
      startTransition(async () => {
        setError(null);
        const r = await review(action);
        if (r.ok) onUpdated(r.data.profile);
        else setError(`${r.title}. ${r.message}`);
        resolve(r.ok);
      }),
    );
  const items = [...profile.understood, ...profile.avoid].flatMap((l) => l.items).concat(profile.unsure, profile.neutral);
  return (
    <SheetBody className="gap-4 pb-6">
      {error && (
        <p role="alert" className="text-[13px] text-danger">
          {error}
        </p>
      )}
      {items.length > 0 ? (
        <ul>
          {items.map((item) => (
            <Correctable key={`${item.id}:${item.polarity}:${item.text}`} item={item} run={run} busy={pending} />
          ))}
        </ul>
      ) : (
        <p className="text-[14px] text-fg-secondary">Nothing yet.</p>
      )}
      <form
        className="border-t border-line-subtle pt-3"
        onSubmit={async (e) => {
          e.preventDefault();
          if (!sentence.trim()) return;
          if (await run({ action: "add", text: sentence.trim() })) setSentence("");
        }}
      >
        <label htmlFor={addId} className={labelClass}>
          Add one sentence
        </label>
        <div className="flex gap-2 max-sm:flex-col">
          <input
            id={addId}
            value={sentence}
            onChange={(e) => setSentence(e.target.value)}
            placeholder="I'd love developer tooling"
            autoComplete="off"
            className={inputClass}
          />
          <Button type="submit" variant="secondary" disabled={pending || !sentence.trim()} className="shrink-0 max-sm:h-11">
            Add
          </Button>
        </div>
      </form>
    </SheetBody>
  );
}

/**
 * "What Narrow understands": 3–6 short lines of what they want, what they
 * avoid, and two actions: Looks right, Edit.
 */
export function TasteSummary({ profile: initial, review }: { profile: TasteProfileView; review: Review }) {
  // The latest answer of a change, until the page brings a newer profile.
  const [latest, setLatest] = useState<{ from: TasteProfileView; profile: TasteProfileView } | null>(null);
  const profile = latest && latest.from === initial ? latest.profile : initial;
  const setProfile = (p: TasteProfileView) => setLatest({ from: initial, profile: p });
  const [editing, setEditing] = useState(false);
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const trigger = useRef<HTMLButtonElement>(null);
  const empty = profile.understood.length === 0 && profile.avoid.length === 0;
  const all = [...profile.understood, ...profile.avoid].flatMap((l) => l.items);
  const interpretation = profile.interpretation;
  return (
    <section id="understood" aria-labelledby="understood-heading" className="scroll-mt-20">
      <div className="mb-1.5 flex items-baseline justify-between gap-4">
        <h2 id="understood-heading" className="text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
          What Narrow understands
        </h2>
        {profile.needs_confirmation ? (
          <span className="text-caption text-fg-muted">Narrow&apos;s reading · check it</span>
        ) : (
          profile.confirmed_at && <span className="text-caption text-fg-muted">You confirmed this</span>
        )}
      </div>
      <div className="border-t border-line-subtle pt-2">
        {empty ? (
          <p className="py-1 text-[14px] text-fg-secondary">
            {profile.looking_for ? "Nothing Narrow could read yet. Say it another way, or add a sentence." : "Tell Narrow what you're looking for, and it will summarize it here."}
          </p>
        ) : (
          <>
            <ul aria-label="What you want">
              {profile.understood.map((l) => (
                <Line key={`${l.dimension}:${l.text}`} line={l} />
              ))}
            </ul>
            {profile.avoid.length > 0 && (
              <>
                <h3 className="mt-3 text-[13px] font-semibold text-fg">You tend to avoid</h3>
                <ul aria-label="What you avoid">
                  {profile.avoid.map((l) => (
                    <Line key={`${l.dimension}:${l.text}`} line={l} />
                  ))}
                </ul>
              </>
            )}
          </>
        )}
        {interpretation && interpretation.ambiguities.length > 0 && (
          <ul className="mt-2 space-y-0.5">
            {interpretation.ambiguities.map((a) => (
              <li key={a} className="text-[13px] text-fg-secondary">
                <span className="nr-inferred">Unclear:</span> {a}
              </li>
            ))}
          </ul>
        )}
        {(profile.looking_for || !empty) && (
          <div className="mt-3 flex flex-wrap items-center gap-x-3 gap-y-2">
            {profile.needs_confirmation && (
              <Button
                variant="primary"
                disabled={pending}
                loading={pending}
                className="max-sm:h-11"
                onClick={() =>
                  startTransition(async () => {
                    setError(null);
                    const r = await review({ action: "confirm" });
                    if (r.ok) setProfile(r.data.profile);
                    else setError(`${r.title}. ${r.message}`);
                  })
                }
              >
                Looks right
              </Button>
            )}
            <button ref={trigger} type="button" aria-haspopup="dialog" onClick={() => setEditing(true)} disabled={pending} className={buttonClass("secondary", "md", "max-sm:h-11")}>
              Edit
            </button>
          </div>
        )}
        {error && (
          <p role="alert" className="mt-2 text-[13px] text-danger">
            {error}
          </p>
        )}
        {(all.length > 0 || interpretation) && (
          <Disclosure label="How Narrow read this" className="mt-3">
            <div className="text-[13px] text-fg-secondary">
              {interpretation && (
                <p>
                  Read by {interpretation.interpreter === "rules/1" ? "Narrow's built-in reader" : interpretation.interpreter}
                  {interpretation.outcome === "fallback" && " (fallback)"}. Nothing here is ever used as something you said unless you said it or confirmed it.
                </p>
              )}
              {interpretation?.note && <p className="mt-1">{interpretation.note}</p>}
              {profile.reader.note && <p className="mt-1">{profile.reader.note}</p>}
              <ul className="mt-2">
                {all.concat(profile.unsure).map((item) => (
                  <Provenance key={item.id} item={item} />
                ))}
              </ul>
              {profile.neutral.length > 0 && <p className="mt-2">Doesn&apos;t matter to you: {profile.neutral.map((n) => n.text.replace(/: doesn't matter$/, "")).join(", ")}.</p>}
              {profile.removed.length > 0 && <p className="mt-1">Removed, never read again: {profile.removed.map((n) => n.text).join(", ")}.</p>}
              {profile.looking_for_source === "description" && (
                <button
                  type="button"
                  disabled={pending}
                  className={`${inlineActionClass} mt-2 max-sm:min-h-11`}
                  onClick={() =>
                    startTransition(async () => {
                      setError(null);
                      const r = await review({ action: "reinterpret" });
                      if (r.ok) setProfile(r.data.profile);
                      else setError(`${r.title}. ${r.message}`);
                    })
                  }
                >
                  Read my words again
                </button>
              )}
            </div>
          </Disclosure>
        )}
      </div>
      <Sheet
        open={editing}
        onClose={() => {
          setEditing(false);
          requestAnimationFrame(() => trigger.current?.focus());
        }}
        title="What Narrow understands"
        subtitle="Change a line, say it doesn't matter, remove it, or add your own. Your corrections always win."
        closeWord
      >
        {editing && <Corrections profile={profile} review={review} onUpdated={setProfile} />}
      </Sheet>
    </section>
  );
}

/** "Practical constraints": whether you can take a job, not whether you'd want it. */
export function Constraints({ items, noted, children }: { items: ConstraintView[]; noted: string[]; children?: ReactNode }) {
  return (
    <section id="constraints" aria-labelledby="constraints-heading" className="scroll-mt-20">
      <h2 id="constraints-heading" className="mb-1.5 text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
        Practical constraints
      </h2>
      <div className="border-t border-line-subtle pt-2">
        {items.length > 0 ? (
          <ul aria-label="Your practical constraints">
            {items.map((c) => (
              <li key={`${c.kind}:${c.text}`} className="flex flex-wrap items-baseline gap-x-2.5 py-1">
                <span className="text-[14px] font-medium text-fg nr-tnum">{c.text}</span>
                {c.layer === "requirement" && <LayerTag layer="requirement" />}
              </li>
            ))}
          </ul>
        ) : (
          <p className="py-1 text-[14px] text-fg-secondary">
            None set{noted.length > 0 ? `. You mentioned ${noted.join(", ")}: set it here if it's a must.` : "."}
          </p>
        )}
        {children && (
          <Disclosure label="Edit constraints" className="mt-2">
            {children}
          </Disclosure>
        )}
      </div>
    </section>
  );
}

/** "Learned over time": only with something learned, and never as something you said. */
export function LearnedOverTime({ items }: { items: TasteItemView[] }) {
  if (items.length === 0) return null;
  return (
    <section id="learned" aria-labelledby="learned-heading" className="scroll-mt-20">
      <h2 id="learned-heading" className="mb-1.5 text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
        Learned over time
      </h2>
      <ul className="border-t border-line-subtle pt-2">
        {items.map((i) => (
          <li key={i.id} className="py-1 text-[14px] text-fg-secondary">
            {i.polarity === "avoid" ? "You tend to pass on " : "You tend to go for "}
            {i.text.charAt(0).toLowerCase() + i.text.slice(1)}
            <span className="text-caption text-fg-muted"> · {i.sources[0]?.text.replace(/^Your feedback: /, "") ?? "from your feedback"}</span>
          </li>
        ))}
      </ul>
      <p className="mt-1.5 text-caption text-fg-muted">From what you saved, passed on and applied to. It only changes the order.</p>
    </section>
  );
}
