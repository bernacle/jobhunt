"use client";

import { type KeyboardEvent, type ReactNode, createContext, useContext, useId, useOptimistic, useState, useTransition } from "react";

import type { ActionResult } from "@/app/actions";
import type {
  CompanyControl,
  PayControl,
  PlaceControl,
  PreferenceControls,
  PreferenceInput,
  PreferenceUpdateResult,
  PreferenceView,
} from "@/lib/api-types";

import { SummaryRow } from "./summary";
import { Button, inlineActionClass, inputClass, labelClass, selectClass } from "./ui";

/*
 * Configuring Narrow's judgment, not filtering a job board. Each row reads
 * the same preference records a sentence in the person's words writes (the
 * API derives the controls from them), and each change sends the same
 * structured inputs the CLI and AI assistants use. Nothing here decides
 * what a value means: the API does, and says which layer each setting is
 * in.
 *
 * By default a row says the current value and offers one action. Editing
 * opens that row's editor in place, one row at a time; the consequence of
 * a choice is said for the selected option only.
 */

export type UpdatePreferences = (set: PreferenceInput[], remove?: string[]) => Promise<ActionResult<PreferenceUpdateResult>>;

type Layer = "requirement" | "preference" | "learned";

const LAYER_TEXT: Record<Layer, string> = {
  requirement: "Requirement",
  preference: "Preference",
  learned: "Learned",
};

/**
 * The three layers' marks: a solid square for a requirement, a half-filled
 * one for a preference, a hollow one in secondary ink for what Narrow
 * learned. The words always go with them.
 */
export function LayerMark({ layer }: { layer: Layer }) {
  const shape =
    layer === "requirement"
      ? "bg-fg"
      : layer === "preference"
        ? "border border-fg bg-[linear-gradient(90deg,var(--nr-fg-primary)_50%,transparent_50%)]"
        : "border border-fg-secondary";
  return <span aria-hidden="true" className={`inline-block size-[7px] shrink-0 rounded-[1px] ${shape}`} />;
}

export function LayerTag({ layer }: { layer?: string | null }) {
  if (layer !== "requirement" && layer !== "preference") return null;
  return (
    <span className="inline-flex items-center gap-1.5 text-caption text-fg-muted">
      <LayerMark layer={layer} />
      {LAYER_TEXT[layer]}
    </span>
  );
}

/**
 * What each layer does, and what happens when a posting doesn't say: the
 * one place these are taught ("How preferences work").
 */
export function LayerTerms() {
  const items: { layer: Layer; term: string; text: ReactNode }[] = [
    {
      layer: "requirement",
      term: "Requirement · Must have",
      text: "A job that states the opposite is left out. If the posting doesn't say, it stays unresolved and is never a Strong fit.",
    },
    { layer: "preference", term: "Preference · Nice to have, Avoid", text: "Changes the order. Never leaves a job out." },
    { layer: "learned", term: "Learned", text: "A tendency from your decisions. Ranking only, and what you say always wins." },
  ];
  return (
    <dl aria-label="How Narrow uses what you tell it" className="flex flex-col gap-5">
      {items.map((i) => (
        <div key={i.layer} className="min-w-0">
          <dt className="flex items-center gap-2 text-[14px] font-semibold text-fg">
            <LayerMark layer={i.layer} />
            {i.term}
          </dt>
          <dd className="mt-0.5 pl-[15px] text-[14px] leading-[1.55] text-fg-body">{i.text}</dd>
        </div>
      ))}
      <div>
        <dt className="text-[14px] font-semibold text-fg">When a posting doesn&apos;t say</dt>
        <dd className="mt-0.5 text-[14px] leading-[1.55] text-fg-body">
          Unknown never counts as meeting a requirement. Pay that isn&apos;t published never meets your minimum.
        </dd>
      </div>
      <div>
        <dt className="text-[14px] font-semibold text-fg">Team, company size and stage</dt>
        <dd className="mt-0.5 text-[14px] leading-[1.55] text-fg-body">
          Three separate things. A company&apos;s size never stands in for your team&apos;s.
        </dd>
      </div>
    </dl>
  );
}

// ---------------------------------------------------------------------------
// One row edited at a time

const Editing = createContext<{ open: string | null; setOpen: (key: string | null) => void } | null>(null);

/** Keeps one preference row open at a time across the page. */
export function PreferenceEditing({ children }: { children: ReactNode }) {
  const [open, setOpen] = useState<string | null>(null);
  return <Editing.Provider value={{ open, setOpen }}>{children}</Editing.Provider>;
}

function useEditing(key: string) {
  const shared = useContext(Editing);
  const [local, setLocal] = useState(false);
  const editing = shared ? shared.open === key : local;
  return {
    editing,
    open: () => (shared ? shared.setOpen(key) : setLocal(true)),
    close: () => (shared ? shared.setOpen(null) : setLocal(false)),
  };
}

/** Runs one change; `onSaved` gets "Saved." or "Already in effect.". */
function useCommit(update: UpdatePreferences) {
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const commit = (set: PreferenceInput[], remove: string[] = [], onSaved?: (message: string) => void, before?: () => void) =>
    startTransition(async () => {
      before?.();
      setError(null);
      const r = await update(set, remove);
      if (r.ok) onSaved?.(r.data.unchanged ? "Already in effect." : "Saved.");
      else setError(`${r.title}. ${r.message}`);
    });
  return { pending, error, setError, commit };
}

type Done = (message?: string) => void;

/**
 * A preference as a summary row, with its editor behind Edit. Escape
 * cancels; closing returns focus to the row's action.
 */
function PreferenceRow({
  rowKey,
  label,
  value,
  unset = false,
  importance,
  actionLabel,
  editor,
}: {
  rowKey: string;
  label: string;
  value: ReactNode;
  unset?: boolean;
  importance?: ReactNode;
  actionLabel?: string;
  editor: (done: Done) => ReactNode;
}) {
  const id = useId();
  const { editing, open, close } = useEditing(rowKey);
  const [status, setStatus] = useState("");
  const triggerId = `${id}-action`;
  const editorId = `${id}-editor`;
  const done: Done = (message) => {
    setStatus(message ?? "");
    close();
    // Back to the row's action once it is on screen again.
    requestAnimationFrame(() => document.getElementById(triggerId)?.focus());
  };
  const onKeyDown = (e: KeyboardEvent) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      done();
    }
  };
  return (
    <SummaryRow
      id={id}
      label={label}
      value={value}
      unset={unset}
      importance={importance}
      status={
        <span role="status" className="text-caption text-fg-muted empty:hidden">
          {status}
        </span>
      }
      action={
        <button
          id={triggerId}
          type="button"
          onClick={() => {
            setStatus("");
            open();
            // Into the editor: the current choice, else its first field.
            requestAnimationFrame(() => {
              const editor = document.getElementById(editorId);
              const target = editor?.querySelector<HTMLElement>("input:checked") ?? editor?.querySelector<HTMLElement>("input, select, textarea, button");
              target?.focus();
            });
          }}
          className={`${inlineActionClass} max-sm:min-h-11 max-sm:pl-3`}
        >
          {actionLabel ?? (unset ? "Add" : "Edit")} <span className="sr-only">{label.toLowerCase()}</span>
        </button>
      }
      editor={
        editing ? (
          <div id={editorId} onKeyDown={onKeyDown} className="pt-0.5 pb-2 max-sm:pt-1.5">
            {editor(done)}
          </div>
        ) : undefined
      }
    />
  );
}

function EditorActions({ pending, onCancel, error, saveLabel = "Save", extra }: { pending: boolean; onCancel: () => void; error?: string | null; saveLabel?: string; extra?: ReactNode }) {
  return (
    <>
      {error && (
        <p role="alert" className="mt-3 text-[13px] text-danger">
          {error}
        </p>
      )}
      <div className="mt-3.5 flex flex-wrap items-center gap-2">
        <Button type="submit" variant="primary" size="sm" disabled={pending} loading={pending} className="max-sm:h-11 max-sm:px-5">
          {saveLabel}
        </Button>
        <Button variant="ghost" size="sm" onClick={onCancel} disabled={pending} className="max-sm:h-11">
          Cancel
        </Button>
        {extra}
      </div>
    </>
  );
}

function DoneButton({ onDone }: { onDone: () => void }) {
  return (
    <Button size="sm" onClick={onDone} className="mt-3.5 max-sm:h-11 max-sm:px-5">
      Done
    </Button>
  );
}

function Outcome({ message }: { message: { ok: boolean; text: string } | null }) {
  return (
    <span role="status" aria-live="polite" className={`text-caption ${message?.ok === false ? "text-danger" : "text-fg-muted"}`}>
      {message?.text}
    </span>
  );
}

interface Option {
  value: string;
  label: string;
  /** The consequence of choosing it, shown once it is selected. */
  note?: string;
}

/**
 * One answer among a few: a quiet list of radio rows, the selected one
 * with its consequence underneath. 48px rows on phones.
 */
function ChoiceList({ name, legend, options, value, onChange }: { name: string; legend: string; options: Option[]; value: string | null; onChange: (value: string) => void }) {
  return (
    <fieldset role="radiogroup" aria-label={legend} className="min-w-0">
      <div className="-mx-2.5 flex max-w-[420px] flex-col gap-0.5">
        {options.map((o) => {
          const on = value === o.value;
          return (
            <div key={o.value}>
              <label
                className={
                  "flex min-h-[34px] cursor-pointer items-center gap-3 rounded-md px-2.5 text-[14px] transition-colors duration-[120ms] max-sm:min-h-12 max-sm:text-[15px] " +
                  "has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) " +
                  (on ? "bg-selected font-medium text-fg" : "text-fg-secondary hover:text-fg")
                }
              >
                <input type="radio" name={name} value={o.value} checked={on} onChange={() => onChange(o.value)} className="sr-only" />
                <span aria-hidden="true" className={`size-[7px] shrink-0 rounded-[1px] ${on ? "bg-fg" : "border border-fg-muted"}`} />
                {o.label}
              </label>
              {on && o.note && <p className="px-2.5 pt-1.5 pb-1 pl-[29px] text-[12.5px] leading-[1.45] text-fg-muted">{o.note}</p>}
            </div>
          );
        })}
      </div>
    </fieldset>
  );
}

/** Must have / Nice to have, and off (and avoid, for kinds of company). */
function Importance({
  name,
  legend,
  value,
  onChange,
  disabled,
  withOff = true,
  withAvoid = false,
}: {
  name: string;
  legend: string;
  value: string;
  onChange: (value: string) => void;
  disabled?: boolean;
  withOff?: boolean;
  withAvoid?: boolean;
}) {
  const options = [
    ...(withOff ? [{ value: "off", label: "Off" }] : []),
    { value: "nice_to_have", label: "Nice to have" },
    { value: "must_have", label: "Must have" },
    ...(withAvoid ? [{ value: "avoid", label: "Avoid" }] : []),
  ];
  return (
    <fieldset role="radiogroup" aria-label={legend} disabled={disabled} className="min-w-0">
      <div className="inline-flex max-w-full flex-wrap rounded-md border border-line p-0.5 max-sm:flex max-sm:flex-nowrap">
        {options.map((o) => (
          <label
            key={o.value}
            className={
              "flex min-h-7 cursor-pointer items-center justify-center rounded-[5px] px-2.5 text-[12.5px] font-medium whitespace-nowrap text-fg-secondary transition-colors duration-[120ms] " +
              "hover:text-fg has-checked:bg-selected has-checked:text-fg has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) max-sm:min-h-11 max-sm:flex-1 max-sm:px-1.5"
            }
          >
            <input type="radio" name={name} value={o.value} checked={value === o.value} onChange={() => onChange(o.value)} className="sr-only" />
            {o.label}
          </label>
        ))}
      </div>
    </fieldset>
  );
}

const IMPORTANCE_TEXT: Record<string, string> = { must_have: "Must have", nice_to_have: "Nice to have", avoid: "Avoid" };

/** A value read from words with an open question: said, not hidden. */
function NeedsAnswer({ records }: { records: (PreferenceView | null | undefined)[] }) {
  if (!records.some((r) => r?.clarify)) return null;
  return <span className="nr-inferred text-[12.5px] font-normal">needs your answer</span>;
}

function listText(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} or ${items[items.length - 1]}`;
}

// ---------------------------------------------------------------------------
// Work

const SETUPS: Option[] = [
  { value: "remote_only", label: "Remote only", note: "Hybrid and on-site roles are left out." },
  { value: "prefer_remote", label: "Prefer remote", note: "Remote roles rank higher. Nothing is left out." },
  { value: "hybrid_okay", label: "Hybrid okay", note: "Remote or hybrid. On-site roles are left out." },
  { value: "onsite_okay", label: "On-site okay", note: "Any setup works for you." },
  { value: "no_preference", label: "No preference", note: "Work setup doesn't count either way." },
];

const RELOCATION: Option[] = [
  { value: "not_willing", label: "Not willing to relocate", note: "Roles that need you somewhere else are left out." },
  { value: "open", label: "Open to relocation", note: "Those roles stay, with the move as a condition." },
  { value: "only_selected", label: "Only to some places", note: "Roles elsewhere are left out." },
];

function WorkSetup({ work, update }: { work: PreferenceControls["work"]; update: UpdatePreferences }) {
  const current = SETUPS.find((s) => s.value === work.setup);
  const unset = work.setup === "no_preference" && work.setup_records.length === 0;
  return (
    <PreferenceRow
      rowKey="work-setup"
      label="Work setup"
      unset={unset}
      value={
        <>
          {work.setup === "custom" ? `From your words: ${work.custom}` : (current?.label ?? "Not set")} <NeedsAnswer records={work.setup_records} />
        </>
      }
      editor={(done) => <WorkSetupEditor work={work} update={update} done={done} />}
    />
  );
}

function WorkSetupEditor({ work, update, done }: { work: PreferenceControls["work"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const id = useId();
  const [choice, setChoice] = useState<string | null>(SETUPS.some((s) => s.value === work.setup) ? work.setup : null);
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (choice) commit([{ kind: "work_setup", setup: choice as "remote_only" }], [], done);
      }}
    >
      {work.setup === "custom" && <p className="mb-2 text-[13px] text-fg-secondary">From your words: {work.custom}. Choose one to replace it.</p>}
      <ChoiceList name={`${id}-setup`} legend="Work setup" options={SETUPS} value={choice} onChange={setChoice} />
      <EditorActions pending={pending} error={error} onCancel={() => done()} />
    </form>
  );
}

function relocationText(work: PreferenceControls["work"]): string {
  switch (work.relocation) {
    case "not_willing":
      return "Not willing to relocate";
    case "open":
      return "Open to relocation";
    case "only_selected":
      return `Only to ${listText(work.relocation_only_to)}`;
    default:
      return "Not set";
  }
}

function Relocation({ work, update }: { work: PreferenceControls["work"]; update: UpdatePreferences }) {
  return (
    <PreferenceRow
      rowKey="relocation"
      label="Relocation"
      unset={work.relocation === "unset"}
      value={
        <>
          {relocationText(work)} <NeedsAnswer records={[work.relocation_record]} />
        </>
      }
      editor={(done) => <RelocationEditor work={work} update={update} done={done} />}
    />
  );
}

function RelocationEditor({ work, update, done }: { work: PreferenceControls["work"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, setError, commit } = useCommit(update);
  const id = useId();
  const [choice, setChoice] = useState<string | null>(work.relocation === "unset" ? null : work.relocation);
  const [places, setPlaces] = useState(work.relocation_only_to.join(", "));
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (choice === "not_willing") commit([{ kind: "relocation", willing: false, only_to: [] }], [], done);
        if (choice === "open") commit([{ kind: "relocation", willing: true, only_to: [] }], [], done);
        if (choice === "only_selected") {
          const list = places
            .split(",")
            .map((p) => p.trim())
            .filter(Boolean);
          if (list.length === 0) return setError("Name at least one country or region.");
          commit([{ kind: "relocation", willing: true, only_to: list }], [], done);
        }
      }}
    >
      <ChoiceList name={`${id}-relocation`} legend="Relocation" options={RELOCATION} value={choice} onChange={setChoice} />
      {choice === "only_selected" && (
        <div className="mt-3 max-w-[420px]">
          <label htmlFor={`${id}-places`} className={labelClass}>
            Countries or regions
          </label>
          <input id={`${id}-places`} value={places} onChange={(e) => setPlaces(e.target.value)} placeholder="Portugal, Spain" className={inputClass} />
        </div>
      )}
      <EditorActions pending={pending} error={error} onCancel={() => done()} />
    </form>
  );
}

// ---------------------------------------------------------------------------
// Location

function Home({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  return (
    <PreferenceRow
      rowKey="home"
      label="Where you live"
      unset={!location.home}
      value={
        location.home ? (
          <>
            {location.home}
            {location.home_basis === "resume" && <span className="font-normal text-fg-muted"> · from your resume</span>}
            {!location.home_country && <span className="font-normal text-fg-secondary"> · not recognized</span>}
          </>
        ) : (
          "Not set"
        )
      }
      editor={(done) => <HomeEditor location={location} update={update} done={done} />}
    />
  );
}

function HomeEditor({ location, update, done }: { location: PreferenceControls["location"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const id = useId();
  const [place, setPlace] = useState(location.home ?? "");
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        if (place.trim()) commit([{ kind: "location", place: place.trim() }], [], done);
      }}
    >
      <label htmlFor={`${id}-home`} className={labelClass}>
        Where you live
      </label>
      <input id={`${id}-home`} value={place} onChange={(e) => setPlace(e.target.value)} placeholder="São Paulo, Brazil" required className={`${inputClass} max-w-[420px]`} />
      <div className="mt-2 max-w-[420px] space-y-1 text-[12.5px] leading-[1.45] text-fg-muted">
        {location.home && location.home_basis === "resume" && <p>From your resume. Save it to make it yours.</p>}
        {location.home && !location.home_country && <p className="text-fg-secondary">Narrow doesn&apos;t recognize this place, so remote scopes can&apos;t be matched to it.</p>}
        {location.remote_open_to_you.length > 0 && (
          <p>
            Remote roles open to {listText(location.remote_open_to_you)} include you. A listing that only says “Remote” doesn&apos;t say where, so it stays
            unresolved.
          </p>
        )}
      </div>
      <EditorActions pending={pending} error={error} onCancel={() => done()} />
    </form>
  );
}

function PlaceChip({ place, onRemove, pending, extra }: { place: PlaceControl; onRemove: () => void; pending: boolean; extra?: ReactNode }) {
  return (
    <li className="inline-flex max-w-full items-center gap-2 rounded-md border border-line px-2.5 py-1 text-[13px] text-fg-body max-sm:min-h-11">
      <span className="min-w-0 truncate">
        {place.place}
        {place.read_as && place.read_as.toLowerCase() !== place.place.toLowerCase() && <span className="text-fg-muted"> · {place.read_as}</span>}
        {!place.read_as && <span className="text-fg-muted"> · not recognized</span>}
        {extra}
      </span>
      <button type="button" disabled={pending} onClick={onRemove} className={`${inlineActionClass} max-sm:min-h-11`}>
        Remove <span className="sr-only">{place.place}</span>
      </button>
    </li>
  );
}

function AddPlace({ label, placeholder, onAdd, pending }: { label: string; placeholder: string; onAdd: (place: string) => void; pending: boolean }) {
  const id = useId();
  const [value, setValue] = useState("");
  return (
    <form
      className="mt-3 flex max-w-[420px] flex-wrap items-end gap-2"
      onSubmit={(e) => {
        e.preventDefault();
        if (value.trim()) {
          onAdd(value.trim());
          setValue("");
        }
      }}
    >
      <div className="min-w-0 flex-1 basis-52">
        <label htmlFor={id} className={labelClass}>
          {label}
        </label>
        <input id={id} value={value} onChange={(e) => setValue(e.target.value)} placeholder={placeholder} className={inputClass} />
      </div>
      <Button type="submit" disabled={pending} className="max-sm:h-11">
        Add
      </Button>
    </form>
  );
}

function placeName(p: PlaceControl): string {
  return p.read_as && p.read_as.toLowerCase() !== p.place.toLowerCase() ? `${p.place} (${p.read_as})` : p.place;
}

function Authorization({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const places = location.authorized_in;
  return (
    <PreferenceRow
      rowKey="authorized"
      label="Authorized to work in"
      unset={places.length === 0}
      value={places.length > 0 ? places.map(placeName).join(", ") : "Not set"}
      editor={(done) => <AuthorizationEditor location={location} update={update} done={done} />}
    />
  );
}

function AuthorizationEditor({ location, update, done }: { location: PreferenceControls["location"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const save = (set: PreferenceInput[], remove: string[] = []) => commit(set, remove, (text) => setMessage({ ok: true, text }));
  return (
    <div>
      {location.authorized_in.length > 0 ? (
        <ul className="flex flex-wrap gap-2">
          {location.authorized_in.map((a) => (
            <PlaceChip key={a.record.id} place={a} pending={pending} onRemove={() => save([], [a.record.id])} />
          ))}
        </ul>
      ) : (
        <p className="text-[12.5px] text-fg-muted">Where a posting requires authorization you haven&apos;t stated, it stays unresolved.</p>
      )}
      <AddPlace label="Add a country or region" placeholder="Brazil, the EU" pending={pending} onAdd={(place) => save([{ kind: "authorized_in", place }])} />
      <p className="mt-2">{error ? <Outcome message={{ ok: false, text: error }} /> : <Outcome message={message} />}</p>
      <DoneButton onDone={() => done()} />
    </div>
  );
}

const SCOPES = [
  { code: "worldwide", value: "Worldwide", label: "Anywhere (no restriction)" },
  { code: "americas", value: "Americas", label: "The Americas" },
  { code: "latam", value: "Latin America", label: "Latin America (LATAM)" },
];

function scopeState(location: PreferenceControls["location"]) {
  const wanted = location.remote_geography.filter((g) => g.record.stance !== "unwanted");
  const avoided = location.remote_geography.filter((g) => g.record.stance === "unwanted");
  const importance = wanted.some((g) => g.record.stance === "required") ? "must_have" : "nice_to_have";
  // Anywhere restricts nothing and ranks nothing up or down; a required list
  // that includes it restricts nothing either.
  const anywhere = wanted.some((g) => g.code === "worldwide");
  const restricting = importance === "must_have" && anywhere ? [] : wanted.filter((g) => g.code !== "worldwide");
  return { wanted, avoided, importance, anywhere, restricting };
}

function RemoteGeography({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const { wanted, avoided, importance, restricting } = scopeState(location);
  const names = wanted.map((g) => (g.code === "worldwide" ? "Anywhere (no restriction)" : placeName(g)));
  const value = [names.join(", "), avoided.length > 0 && `avoid ${avoided.map(placeName).join(", ")}`].filter(Boolean).join("; ");
  return (
    <PreferenceRow
      rowKey="remote-scope"
      label="Remote roles open to"
      unset={location.remote_geography.length === 0}
      value={value || "Not set"}
      importance={restricting.length > 0 ? IMPORTANCE_TEXT[importance] : undefined}
      editor={(done) => <RemoteGeographyEditor location={location} update={update} done={done} />}
    />
  );
}

function RemoteGeographyEditor({ location, update, done }: { location: PreferenceControls["location"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const id = useId();
  const save = (set: PreferenceInput[], remove: string[] = []) => commit(set, remove, (text) => setMessage({ ok: true, text }));
  const { wanted, avoided, importance, anywhere } = scopeState(location);
  const stance = importance === "must_have" ? "require" : "want";
  const presetOf = (code: string) => wanted.find((g) => g.code === code);
  const others = wanted.filter((g) => !SCOPES.some((s) => s.code === g.code));
  return (
    <div>
      <fieldset disabled={pending}>
        <legend className="sr-only">Remote scopes</legend>
        <div className="flex flex-wrap gap-2">
          {SCOPES.map((s) => {
            const record = presetOf(s.code);
            return (
              <label
                key={s.code}
                className={
                  "flex min-h-9 cursor-pointer items-center gap-2 rounded-md border border-line px-3 text-[13px] text-fg-body transition-colors duration-[120ms] " +
                  "hover:border-line-strong has-checked:border-fg has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) max-sm:min-h-11"
                }
              >
                <input
                  type="checkbox"
                  checked={Boolean(record)}
                  onChange={() => (record ? save([], [record.record.id]) : save([{ kind: "region", region: s.value, stance }]))}
                  className="size-3.5 accent-fg"
                />
                {s.label}
              </label>
            );
          })}
        </div>
      </fieldset>
      {anywhere && <p className="mt-2 text-[12.5px] text-fg-muted">Anywhere means no geographic restriction: it doesn&apos;t rank remote roles up or down.</p>}
      {others.length > 0 && (
        <ul aria-label="Other places" className="mt-3 flex flex-wrap gap-2">
          {others.map((g) => (
            <PlaceChip key={g.record.id} place={g} pending={pending} onRemove={() => save([], [g.record.id])} />
          ))}
        </ul>
      )}
      <AddPlace label="Add a country or region" placeholder="Brazil, Europe" pending={pending} onAdd={(region) => save([{ kind: "region", region, stance }])} />
      {wanted.length > 0 && (
        <div className="mt-3 flex flex-wrap items-center gap-3">
          <span className="text-caption text-fg-muted">How much it matters</span>
          <Importance
            name={`${id}-importance`}
            legend="How much remote scope matters"
            value={importance}
            withOff={false}
            disabled={pending}
            onChange={(value) =>
              save(wanted.map((g) => ({ kind: "region" as const, region: g.place, stance: value === "must_have" ? ("require" as const) : ("want" as const) })))
            }
          />
        </div>
      )}
      {avoided.length > 0 && (
        <ul aria-label="Places you'd rather avoid" className="mt-3 flex flex-wrap gap-2">
          {avoided.map((g) => (
            <PlaceChip key={g.record.id} place={g} pending={pending} onRemove={() => save([], [g.record.id])} extra={<span className="text-fg-muted"> · avoid</span>} />
          ))}
        </ul>
      )}
      <p className="mt-2">{error ? <Outcome message={{ ok: false, text: error }} /> : <Outcome message={message} />}</p>
      <DoneButton onDone={() => done()} />
    </div>
  );
}

const POLICY: Record<"eligibility" | "pay", Option[]> = {
  eligibility: [
    { value: "show", label: "Show them, marked unresolved", note: "You decide once you know more." },
    { value: "hide", label: "Only when it's confirmed", note: "Left out until Narrow can confirm it." },
  ],
  pay: [
    { value: "show", label: "Show them, marked unresolved", note: "Unknown pay never counts as meeting your minimum." },
    { value: "hide", label: "Hide them", note: "Only roles that publish comparable pay." },
  ],
};

/** What to do when something can't be settled: show it unresolved, or leave it out. */
function Policy({
  rowKey,
  label,
  kind,
  value,
  update,
}: {
  rowKey: string;
  label: string;
  kind: "eligibility" | "pay";
  value: string;
  update: UpdatePreferences;
}) {
  const options = POLICY[kind];
  return (
    <PreferenceRow
      rowKey={rowKey}
      label={label}
      actionLabel="Change"
      value={options.find((o) => o.value === value)?.label ?? value}
      editor={(done) => <PolicyEditor label={label} kind={kind} value={value} update={update} done={done} />}
    />
  );
}

function PolicyEditor({ label, kind, value, update, done }: { label: string; kind: "eligibility" | "pay"; value: string; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const id = useId();
  const [choice, setChoice] = useState(value);
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const show = choice === "show";
        commit([kind === "pay" ? { kind: "unknown_pay", show } : { kind: "unclear_eligibility", show }], [], done);
      }}
    >
      <ChoiceList name={`${id}-policy`} legend={label} options={POLICY[kind]} value={choice} onChange={setChoice} />
      <EditorActions pending={pending} error={error} onCancel={() => done()} />
    </form>
  );
}

// ---------------------------------------------------------------------------
// Pay

const CURRENCIES = ["USD", "EUR", "GBP", "BRL", "CAD", "AUD", "CHF"];

/** Every pay period the API stores; a figure is never converted. */
type Period = "year" | "month" | "day" | "hour";
const PERIODS: { value: Period; label: string }[] = [
  { value: "year", label: "Year" },
  { value: "month", label: "Month" },
  { value: "day", label: "Day" },
  { value: "hour", label: "Hour" },
];

function periodOf(value: string | undefined): Period {
  return PERIODS.find((p) => p.value === value)?.value ?? "year";
}

function payText(p: PayControl): string {
  const amount = p.amount.toLocaleString("en-US");
  const who = p.applies_to ? ` (${p.applies_to} only)` : "";
  return p.currency ? `${p.currency} ${amount} per ${p.period}${who}` : `${amount} per ${p.period}, currency not set${who}`;
}

function PayRow({ bound, records, update }: { bound: "minimum" | "target"; records: PayControl[]; update: UpdatePreferences }) {
  // The figure for both arrangements is the one edited here.
  const main = records.find((r) => !r.applies_to);
  const minimum = bound === "minimum";
  return (
    <PreferenceRow
      rowKey={`pay-${bound}`}
      label={minimum ? "Minimum" : "Target"}
      unset={records.length === 0}
      value={
        records.length > 0 ? (
          <>
            {main && `${minimum ? "At least" : "Around"} ${payText(main)}`}
            {records
              .filter((r) => r.applies_to)
              .map((r, i) => (
                <span key={r.record.id} className="font-normal text-fg-secondary">
                  {main || i > 0 ? "; " : ""}
                  {payText(r)}
                </span>
              ))}{" "}
            <NeedsAnswer records={records.map((r) => r.record)} />
          </>
        ) : (
          "Not set"
        )
      }
      editor={(done) => <PayEditor bound={bound} records={records} update={update} done={done} />}
    />
  );
}

function PayEditor({ bound, records, update, done }: { bound: "minimum" | "target"; records: PayControl[]; update: UpdatePreferences; done: Done }) {
  const { pending, error: saveError, commit } = useCommit(update);
  const id = useId();
  const main = records.find((r) => !r.applies_to);
  const others = records.filter((r) => r.applies_to);
  const [amount, setAmount] = useState(main ? String(main.amount) : "");
  const [currency, setCurrency] = useState(main?.currency ?? "");
  const [period, setPeriod] = useState<Period>(periodOf(main?.period));
  const [error, setError] = useState<string | null>(null);
  const minimum = bound === "minimum";
  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        setError(null);
        const value = Number(amount.replace(/[,_\s]/g, ""));
        const code = currency.trim().toUpperCase();
        if (!Number.isFinite(value) || value <= 0) return setError("Enter the amount as a number, like 140000.");
        if (!/^[A-Z]{3}$/.test(code)) return setError("Choose the currency: a three-letter code like USD or EUR. Narrow never assumes one.");
        commit(
          [
            {
              kind: "compensation",
              minimum: minimum ? value : null,
              target: minimum ? null : value,
              currency: code,
              period,
              applies_to: null,
            },
          ],
          [],
          done,
        );
      }}
    >
      <div className="grid max-w-[460px] gap-2 grid-cols-[minmax(0,1fr)_96px] sm:grid-cols-[minmax(0,1fr)_88px_112px]">
        <div className="min-w-0 max-sm:col-span-2">
          <label htmlFor={`${id}-amount`} className={labelClass}>
            Amount
          </label>
          <input id={`${id}-amount`} value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="numeric" placeholder="140,000" className={inputClass} />
        </div>
        <div className="min-w-0">
          <label htmlFor={`${id}-currency`} className={labelClass}>
            Currency
          </label>
          <input
            id={`${id}-currency`}
            value={currency}
            onChange={(e) => setCurrency(e.target.value)}
            list={`${id}-currencies`}
            maxLength={3}
            autoComplete="off"
            placeholder="USD"
            className={`${inputClass} uppercase`}
          />
          <datalist id={`${id}-currencies`}>
            {CURRENCIES.map((c) => (
              <option key={c} value={c} />
            ))}
          </datalist>
        </div>
        <div className="min-w-0">
          <label htmlFor={`${id}-period`} className={labelClass}>
            Per
          </label>
          <select id={`${id}-period`} value={period} onChange={(e) => setPeriod(periodOf(e.target.value))} className={selectClass}>
            {PERIODS.map((p) => (
              <option key={p.value} value={p.value}>
                {p.label}
              </option>
            ))}
          </select>
        </div>
      </div>
      <p className="mt-2 text-[12.5px] text-fg-muted">
        {minimum ? "Verified pay below it is left out. Pay that isn't published never counts as meeting it." : "Changes the order. Leaves nothing out."}
      </p>
      {others.length > 0 && (
        <ul className="mt-3 max-w-[460px] space-y-1 text-[13px] text-fg-body">
          {others.map((o) => (
            <li key={o.record.id} className="flex items-baseline justify-between gap-3">
              <span className="nr-tnum">{payText(o)}</span>
              <button type="button" disabled={pending} onClick={() => commit([], [o.record.id], done)} className={`${inlineActionClass} max-sm:min-h-11`}>
                Remove <span className="sr-only">{payText(o)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <EditorActions
        pending={pending}
        error={error ?? saveError}
        onCancel={() => done()}
        extra={
          main && (
            <button type="button" disabled={pending} onClick={() => commit([], [main.record.id], () => done("Removed."))} className={`${inlineActionClass} ml-2 max-sm:min-h-11`}>
              Remove <span className="sr-only">{bound}</span>
            </button>
          )
        }
      />
    </form>
  );
}

// ---------------------------------------------------------------------------
// Company and team

const KINDS: Record<string, { label: string; hint: string }> = {
  small_team: { label: "Small team", hint: "The team you'd join, whatever the company's size." },
  small_company: { label: "Small company", hint: "The whole company's headcount." },
  large_company: { label: "Large company", hint: "The whole company's headcount." },
  early_stage: { label: "Early-stage", hint: "Seed to Series A." },
  startup: { label: "Startup", hint: "" },
  scaleup: { label: "Scale-up", hint: "Growth stage." },
};

const SCOPE_TITLE: Record<string, string> = {
  team: "Team",
  company_size: "Company size",
  stage: "Stage",
};

function CompanyScope({ scope, items, update }: { scope: string; items: CompanyControl[]; update: UpdatePreferences }) {
  const on = items.filter((i) => i.importance !== "off");
  const title = SCOPE_TITLE[scope] ?? scope;
  return (
    <PreferenceRow
      rowKey={`company-${scope}`}
      label={title}
      unset={on.length === 0}
      value={
        on.length > 0 ? (
          <>
            {on.map((i, n) => (
              <span key={i.value}>
                {n > 0 && "; "}
                {KINDS[i.value]?.label ?? i.value}
                <span className="font-normal text-fg-muted"> · {IMPORTANCE_TEXT[i.importance] ?? i.importance}</span>
              </span>
            ))}{" "}
            <NeedsAnswer records={on.map((i) => i.record)} />
          </>
        ) : (
          "No preference"
        )
      }
      editor={(done) => (
        <div>
          <ul className="max-w-[460px] divide-y divide-line-subtle">
            {items.map((i) => (
              <CompanyKind key={i.value} item={i} update={update} />
            ))}
          </ul>
          <DoneButton onDone={() => done()} />
        </div>
      )}
    />
  );
}

function CompanyKind({ item, update }: { item: CompanyControl; update: UpdatePreferences }) {
  const { pending, error, commit } = useCommit(update);
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  const id = useId();
  const kind = KINDS[item.value] ?? { label: item.value, hint: "" };
  // Shows the choice at once; the refreshed controls replace it, and a refusal puts it back.
  const [shown, setShown] = useOptimistic(item.importance);
  const saved = (text: string) => setMessage({ ok: true, text });
  return (
    <li>
      <div role="group" aria-labelledby={id} className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 py-2.5 first:pt-0">
        <div className="min-w-0">
          <p id={id} className="text-[13.5px] font-medium text-fg">
            {kind.label}
          </p>
          {kind.hint && <p className="text-caption text-fg-muted">{kind.hint}</p>}
          <p className="empty:hidden">{error ? <Outcome message={{ ok: false, text: error }} /> : message && <Outcome message={message} />}</p>
        </div>
        <Importance
          name={`${id}-importance`}
          legend="How much it matters"
          value={shown}
          withAvoid
          disabled={pending}
          onChange={(value) => {
            if (value === "off") {
              if (item.record) commit([], [item.record.id], saved, () => setShown(value));
              return;
            }
            const stance = value === "must_have" ? "require" : value === "avoid" ? "avoid" : "want";
            commit([{ kind: "company", company: item.value, stance }], [], saved, () => setShown(value));
          }}
        />
      </div>
    </li>
  );
}

// ---------------------------------------------------------------------------
// The groups

export function WorkControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <>
      <WorkSetup work={controls.work} update={update} />
      <Relocation work={controls.work} update={update} />
    </>
  );
}

export function LocationControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <>
      <Home location={controls.location} update={update} />
      <Authorization location={controls.location} update={update} />
      <RemoteGeography location={controls.location} update={update} />
      <Policy rowKey="unclear-eligibility" label="When eligibility is unclear" kind="eligibility" value={controls.location.unclear_eligibility} update={update} />
    </>
  );
}

export function PayControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <>
      <PayRow bound="minimum" records={controls.pay.minimum} update={update} />
      <PayRow bound="target" records={controls.pay.target} update={update} />
      <Policy rowKey="unknown-pay" label="When pay isn't published" kind="pay" value={controls.pay.unknown_pay} update={update} />
    </>
  );
}

export function CompanyControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <>
      {["team", "company_size", "stage"].map((scope) => (
        <CompanyScope key={scope} scope={scope} items={controls.company.items.filter((i) => i.scope === scope)} update={update} />
      ))}
    </>
  );
}
