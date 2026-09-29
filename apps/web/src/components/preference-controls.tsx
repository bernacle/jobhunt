"use client";

import { type Dispatch, type ReactNode, type SetStateAction, createContext, useContext, useId, useState, useTransition } from "react";

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

import { Sheet, SheetBody, SheetFooter } from "./sheet";
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
 * By default a row says the current value, which layer it is in, and one
 * action. Editing opens one decision in a focused sheet, as a draft: Save
 * sends it in one change, Cancel leaves everything as it was. The
 * consequence of a choice is said for the selected option only, and what
 * happens when a posting doesn't say only where a setting behaves
 * differently then.
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
// One decision edited at a time

const Editing = createContext<{ open: string | null; setOpen: Dispatch<SetStateAction<string | null>> } | null>(null);

/** Keeps one preference open for editing at a time across the page. */
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
    // Only its own: a save that finishes after the person moved on to
    // another decision doesn't close that one.
    close: () => (shared ? shared.setOpen((current) => (current === key ? null : current)) : setLocal(false)),
  };
}

/** Runs one change; `onSaved` gets "Saved." or "Already in effect.". */
function useCommit(update: UpdatePreferences) {
  const [pending, startTransition] = useTransition();
  const [error, setError] = useState<string | null>(null);
  const commit = (set: PreferenceInput[], remove: string[] = [], onSaved?: (message: string) => void) =>
    startTransition(async () => {
      setError(null);
      const r = await update(set, remove);
      if (r.ok) onSaved?.(r.data.unchanged ? "Already in effect." : "Saved.");
      else setError(`${r.title}. ${r.message}`);
    });
  return { pending, error, setError, commit };
}

type Done = (message?: string) => void;

/** Into the editor: the current choice, else its first field. */
function focusChoice(panel: HTMLElement) {
  return panel.querySelector<HTMLElement>("form input:checked") ?? panel.querySelector<HTMLElement>("form input, form select, form textarea");
}

/**
 * A preference as a summary row that never changes shape. Its action (the
 * whole row is the target) opens one focused editor: a panel at the side
 * on wide screens, a sheet on phones, titled with the decision it is
 * about and the value it has now. Nothing else on the page can be changed
 * meanwhile. Save or Cancel (or Escape) returns to the row, with focus on
 * its action and the outcome beside its value.
 */
function PreferenceRow({
  rowKey,
  label,
  question,
  now,
  value,
  unset = false,
  importance,
  actionLabel,
  editor,
}: {
  rowKey: string;
  label: string;
  /** The decision, as the person would ask it ("How do you want to work?"). */
  question: string;
  /** The current value in plain words, under the editor's title. */
  now: string;
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
  const done: Done = (message) => {
    setStatus(message ?? "");
    close();
    // Back to the row's action.
    requestAnimationFrame(() => document.getElementById(triggerId)?.focus());
  };
  return (
    <>
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
            aria-haspopup="dialog"
            aria-expanded={editing}
            onClick={() => {
              setStatus("");
              open();
            }}
            // The whole row opens it; the word is where the eye goes.
            className={`${inlineActionClass} after:absolute after:inset-0 after:content-[''] max-sm:min-h-11 max-sm:pl-3`}
          >
            {actionLabel ?? (unset ? "Add" : "Edit")} <span className="sr-only">{label.toLowerCase()}</span>
          </button>
        }
      />
      <Sheet
        open={editing}
        onClose={() => done()}
        title={question}
        subtitle={
          <>
            {label} · {unset ? "Not set" : `Now: ${now}`}
          </>
        }
        fit
        closeWord={false}
        initialFocus={focusChoice}
      >
        {editing && editor(done)}
      </Sheet>
    </>
  );
}

/**
 * An editor's form: its fields in the sheet's body, then Cancel and Save
 * at the foot, always in view. An error stays next to them, with what the
 * person entered still in place.
 */
export { focusChoice };

export function EditorForm({
  onSubmit,
  onCancel,
  pending,
  error,
  saveLabel = "Save",
  extra,
  children,
}: {
  onSubmit: () => void;
  onCancel?: () => void;
  pending: boolean;
  error?: string | null;
  saveLabel?: string;
  /** A secondary action on the left ("Remove minimum"). */
  extra?: ReactNode;
  children: ReactNode;
}) {
  return (
    <form
      noValidate
      onSubmit={(e) => {
        e.preventDefault();
        onSubmit();
      }}
      className="flex min-h-0 flex-1 flex-col"
    >
      <SheetBody className="gap-5 pb-6">{children}</SheetBody>
      <SheetFooter>
        {error && (
          <p role="alert" className="w-full text-[13px] leading-[1.45] text-danger">
            {error}
          </p>
        )}
        {extra && <div className="mr-auto max-sm:w-full">{extra}</div>}
        <div className={`ml-auto flex gap-2 max-sm:grid max-sm:w-full ${onCancel ? "max-sm:grid-cols-[1fr_1.4fr]" : "max-sm:grid-cols-1"}`}>
          {onCancel && (
            <Button variant="ghost" onClick={onCancel} disabled={pending} className="max-sm:h-11">
              Cancel
            </Button>
          )}
          <Button type="submit" variant="primary" disabled={pending} loading={pending} className="max-sm:h-11">
            {saveLabel}
          </Button>
        </div>
      </SheetFooter>
    </form>
  );
}

/** What a choice does, in one line. */
export function Note({ children, className = "" }: { children: ReactNode; className?: string }) {
  return <p className={`text-[12.5px] leading-[1.5] text-pretty text-fg-muted ${className}`}>{children}</p>;
}

/**
 * What happens when a posting doesn't say, marked with the hollow square
 * Narrow uses for an unknown. Only where a setting behaves differently
 * then; the full rules are in "How preferences work".
 */
function IfUnknown({ children }: { children: ReactNode }) {
  return (
    <p className="flex gap-2.5 text-[12.5px] leading-[1.5] text-fg-muted">
      <span aria-hidden="true" className="mt-[0.5em] size-[5px] shrink-0 rounded-[1px] border border-fg-muted" />
      <span className="min-w-0 text-pretty">{children}</span>
    </p>
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
      <div className="-mx-2.5 flex flex-col gap-0.5">
        {options.map((o) => {
          const on = value === o.value;
          return (
            <div key={o.value}>
              <label
                className={
                  "flex min-h-[36px] cursor-pointer items-center gap-3 rounded-md px-2.5 text-[14px] transition-colors duration-[120ms] max-sm:min-h-12 max-sm:text-[15px] " +
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
    <fieldset role="radiogroup" aria-label={legend} disabled={disabled} className="min-w-0 max-sm:w-full">
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
// Places, changed as a draft and saved together

interface Chip {
  key: string;
  text: string;
  note?: string;
}

/** Places in the draft, each removable. Wrapping, never wider than the sheet. */
function PlaceChips({ label, items, onRemove, disabled }: { label: string; items: Chip[]; onRemove: (key: string) => void; disabled?: boolean }) {
  if (items.length === 0) return null;
  return (
    <ul aria-label={label} className="flex flex-wrap gap-2">
      {items.map((c) => (
        <li key={c.key} className="inline-flex max-w-full items-center gap-2 rounded-md border border-line py-1 pr-1.5 pl-2.5 text-[13px] text-fg-body max-sm:min-h-11">
          <span className="min-w-0 [overflow-wrap:anywhere]">
            {c.text}
            {c.note && <span className="text-fg-muted"> · {c.note}</span>}
          </span>
          <button type="button" disabled={disabled} onClick={() => onRemove(c.key)} className={`${inlineActionClass} shrink-0 px-1 max-sm:min-h-11`}>
            Remove <span className="sr-only">{c.text}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

/**
 * A place to add to the draft. Enter adds it; a place typed but not yet
 * added is saved with the rest.
 */
function AddPlace({
  label,
  placeholder,
  value,
  onChange,
  onAdd,
  disabled,
}: {
  label: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
  onAdd: () => void;
  disabled?: boolean;
}) {
  const id = useId();
  return (
    <div className="flex flex-wrap items-end gap-2">
      <div className="min-w-0 flex-1 basis-52">
        <label htmlFor={id} className={labelClass}>
          {label}
        </label>
        <input
          id={id}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              onAdd();
            }
          }}
          placeholder={placeholder}
          autoComplete="off"
          className={inputClass}
        />
      </div>
      <Button onClick={onAdd} disabled={disabled || !value.trim()} className="h-9 max-sm:h-11">
        Add
      </Button>
    </div>
  );
}

/** The draft's new places: those added, and the one still typed. */
function withTyped(added: string[], typed: string): string[] {
  const t = typed.trim();
  const all = t ? [...added, t] : added;
  return all.filter((p, i) => all.findIndex((q) => q.toLowerCase() === p.toLowerCase()) === i);
}

function readAs(p: PlaceControl): string | undefined {
  if (!p.read_as) return "not recognized";
  return p.read_as.toLowerCase() !== p.place.toLowerCase() ? p.read_as : undefined;
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

function setupText(work: PreferenceControls["work"]): string {
  if (work.setup === "custom") return `From your words: ${work.custom}`;
  return SETUPS.find((s) => s.value === work.setup)?.label ?? "Not set";
}

function WorkSetup({ work, update }: { work: PreferenceControls["work"]; update: UpdatePreferences }) {
  const unset = work.setup === "no_preference" && work.setup_records.length === 0;
  return (
    <PreferenceRow
      rowKey="work-setup"
      label="Work setup"
      question="How do you want to work?"
      now={setupText(work)}
      unset={unset}
      value={
        <>
          {setupText(work)} <NeedsAnswer records={work.setup_records} />
        </>
      }
      importance={unset ? undefined : <LayerTag layer={work.setup_layer} />}
      editor={(done) => <WorkSetupEditor work={work} update={update} done={done} />}
    />
  );
}

function WorkSetupEditor({ work, update, done }: { work: PreferenceControls["work"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, setError, commit } = useCommit(update);
  const id = useId();
  const [choice, setChoice] = useState<string | null>(SETUPS.some((s) => s.value === work.setup) ? work.setup : null);
  return (
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        if (!choice) return setError("Choose one of the answers.");
        commit([{ kind: "work_setup", setup: choice as "remote_only" }], [], done);
      }}
    >
      {work.setup === "custom" && <Note className="text-fg-secondary">From your words: {work.custom}. Choose one to replace it.</Note>}
      <ChoiceList name={`${id}-setup`} legend="Work setup" options={SETUPS} value={choice} onChange={setChoice} />
    </EditorForm>
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
  const unset = work.relocation === "unset";
  return (
    <PreferenceRow
      rowKey="relocation"
      label="Relocation"
      question="Would you move for a role?"
      now={relocationText(work)}
      unset={unset}
      value={
        <>
          {relocationText(work)} <NeedsAnswer records={[work.relocation_record]} />
        </>
      }
      importance={unset ? undefined : <LayerTag layer={work.relocation_record?.layer} />}
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
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        if (choice === "not_willing") return commit([{ kind: "relocation", willing: false, only_to: [] }], [], done);
        if (choice === "open") return commit([{ kind: "relocation", willing: true, only_to: [] }], [], done);
        if (choice === "only_selected") {
          const list = places
            .split(",")
            .map((p) => p.trim())
            .filter(Boolean);
          if (list.length === 0) return setError("Name at least one country or region.");
          return commit([{ kind: "relocation", willing: true, only_to: list }], [], done);
        }
        setError("Choose one of the answers.");
      }}
    >
      <ChoiceList name={`${id}-relocation`} legend="Relocation" options={RELOCATION} value={choice} onChange={setChoice} />
      {choice === "only_selected" && (
        <div>
          <label htmlFor={`${id}-places`} className={labelClass}>
            Countries or regions
          </label>
          <input id={`${id}-places`} value={places} onChange={(e) => setPlaces(e.target.value)} placeholder="Portugal, Spain" className={inputClass} />
        </div>
      )}
    </EditorForm>
  );
}

// ---------------------------------------------------------------------------
// Location

function Home({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  return (
    <PreferenceRow
      rowKey="home"
      label="Where you live"
      question="Where do you live?"
      now={location.home ?? ""}
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
  const { pending, error, setError, commit } = useCommit(update);
  const id = useId();
  const [place, setPlace] = useState(location.home ?? "");
  return (
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        if (!place.trim()) return setError("Enter where you live, like a city and a country.");
        commit([{ kind: "location", place: place.trim() }], [], done);
      }}
    >
      <div>
        <label htmlFor={`${id}-home`} className={labelClass}>
          Where you live
        </label>
        <input id={`${id}-home`} value={place} onChange={(e) => setPlace(e.target.value)} placeholder="São Paulo, Brazil" autoComplete="off" className={inputClass} />
      </div>
      <div className="flex flex-col gap-1.5">
        {location.home && location.home_basis === "resume" && <Note>From your resume. Save it to make it yours.</Note>}
        {location.home && !location.home_country && (
          <Note className="text-fg-secondary">Narrow doesn&apos;t recognize this place, so remote scopes can&apos;t be matched to it.</Note>
        )}
        {location.remote_open_to_you.length > 0 && <Note>Remote roles open to {listText(location.remote_open_to_you)} include you.</Note>}
        <IfUnknown>A listing that only says “Remote” doesn&apos;t say where, so it stays unresolved.</IfUnknown>
      </div>
    </EditorForm>
  );
}

function placeName(p: PlaceControl): string {
  return p.read_as && p.read_as.toLowerCase() !== p.place.toLowerCase() ? `${p.place} (${p.read_as})` : p.place;
}

function Authorization({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const places = location.authorized_in;
  const text = places.map(placeName).join(", ");
  return (
    <PreferenceRow
      rowKey="authorized"
      label="Authorized to work in"
      question="Where are you authorized to work?"
      now={text}
      unset={places.length === 0}
      value={places.length > 0 ? text : "Not set"}
      editor={(done) => <AuthorizationEditor location={location} update={update} done={done} />}
    />
  );
}

function AuthorizationEditor({ location, update, done }: { location: PreferenceControls["location"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const [kept, setKept] = useState(location.authorized_in);
  const [added, setAdded] = useState<string[]>([]);
  const [typed, setTyped] = useState("");
  const chips: Chip[] = [
    ...kept.map((a) => ({ key: a.record.id, text: a.place, note: readAs(a) })),
    ...added.map((p) => ({ key: `new:${p}`, text: p })),
  ];
  return (
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        const places = withTyped(added, typed);
        const remove = location.authorized_in.filter((a) => !kept.includes(a)).map((a) => a.record.id);
        if (places.length === 0 && remove.length === 0) return done();
        commit(
          places.map((place) => ({ kind: "authorized_in" as const, place })),
          remove,
          done,
        );
      }}
    >
      <PlaceChips
        label="Authorized to work in"
        items={chips}
        disabled={pending}
        onRemove={(key) => (key.startsWith("new:") ? setAdded(added.filter((p) => `new:${p}` !== key)) : setKept(kept.filter((a) => a.record.id !== key)))}
      />
      <AddPlace
        label="Add a country or region"
        placeholder="Brazil, the EU"
        value={typed}
        onChange={setTyped}
        disabled={pending}
        onAdd={() => {
          setAdded(withTyped(added, typed));
          setTyped("");
        }}
      />
      <IfUnknown>Where a posting requires authorization you haven&apos;t stated, it stays unresolved.</IfUnknown>
    </EditorForm>
  );
}

const SCOPES = [
  { code: "worldwide", value: "Worldwide", label: "Anywhere (no restriction)" },
  { code: "americas", value: "Americas", label: "The Americas" },
  { code: "latam", value: "Latin America", label: "Latin America (LATAM)" },
];

const isPreset = (g: PlaceControl) => SCOPES.some((s) => s.code === g.code);

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

function scopeText(location: PreferenceControls["location"]): string {
  const { wanted, avoided } = scopeState(location);
  const names = wanted.map((g) => (g.code === "worldwide" ? "Anywhere (no restriction)" : placeName(g)));
  return [names.join(", "), avoided.length > 0 && `avoid ${avoided.map(placeName).join(", ")}`].filter(Boolean).join("; ");
}

function RemoteGeography({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const { importance, restricting } = scopeState(location);
  const text = scopeText(location);
  return (
    <PreferenceRow
      rowKey="remote-scope"
      label="Remote roles open to"
      question="Which remote regions work for you?"
      now={text}
      unset={location.remote_geography.length === 0}
      value={text || "Not set"}
      importance={restricting.length > 0 ? <LayerTag layer={importance === "must_have" ? "requirement" : "preference"} /> : undefined}
      editor={(done) => <RemoteGeographyEditor location={location} update={update} done={done} />}
    />
  );
}

function RemoteGeographyEditor({ location, update, done }: { location: PreferenceControls["location"]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const id = useId();
  const initial = scopeState(location);
  const presets = initial.wanted.filter(isPreset);
  const [checked, setChecked] = useState(() => new Set(presets.map((g) => g.code!)));
  const [others, setOthers] = useState(initial.wanted.filter((g) => !isPreset(g)));
  const [avoided, setAvoided] = useState(initial.avoided);
  const [added, setAdded] = useState<string[]>([]);
  const [typed, setTyped] = useState("");
  const [importance, setImportance] = useState(initial.importance);
  const anyWanted = checked.size > 0 || others.length > 0 || withTyped(added, typed).length > 0;
  const anywhere = checked.has("worldwide") || others.some((g) => g.code === "worldwide");
  const chips: Chip[] = [
    ...others.map((g) => ({ key: g.record.id, text: g.place, note: readAs(g) })),
    ...added.map((p) => ({ key: `new:${p}`, text: p })),
  ];
  return (
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        const stance = importance === "must_have" ? ("require" as const) : ("want" as const);
        const remove = [
          ...presets.filter((g) => !checked.has(g.code!)),
          ...initial.wanted.filter((g) => !isPreset(g) && !others.includes(g)),
          ...initial.avoided.filter((g) => !avoided.includes(g)),
        ].map((g) => g.record.id);
        // A new importance applies to every place kept; new places get it too.
        const kept = importance !== initial.importance ? [...presets.filter((g) => checked.has(g.code!)), ...others].map((g) => g.place) : [];
        const newPresets = SCOPES.filter((s) => checked.has(s.code) && !presets.some((g) => g.code === s.code)).map((s) => s.value);
        const set = [...kept, ...newPresets, ...withTyped(added, typed)].map((region) => ({ kind: "region" as const, region, stance }));
        if (set.length === 0 && remove.length === 0) return done();
        commit(set, remove, done);
      }}
    >
      <fieldset disabled={pending} className="min-w-0">
        <legend className="sr-only">Remote scopes</legend>
        <div className="flex flex-wrap gap-2">
          {SCOPES.map((s) => (
            <label
              key={s.code}
              className={
                "flex min-h-9 cursor-pointer items-center gap-2 rounded-md border border-line px-3 text-[13px] text-fg-body transition-colors duration-[120ms] " +
                "hover:border-line-strong has-checked:border-fg has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) max-sm:min-h-11"
              }
            >
              <input
                type="checkbox"
                checked={checked.has(s.code)}
                onChange={() => {
                  const next = new Set(checked);
                  if (next.has(s.code)) next.delete(s.code);
                  else next.add(s.code);
                  setChecked(next);
                }}
                className="size-3.5 accent-fg"
              />
              {s.label}
            </label>
          ))}
        </div>
      </fieldset>
      <PlaceChips
        label="Other places"
        items={chips}
        disabled={pending}
        onRemove={(key) => (key.startsWith("new:") ? setAdded(added.filter((p) => `new:${p}` !== key)) : setOthers(others.filter((g) => g.record.id !== key)))}
      />
      <AddPlace
        label="Add a country or region"
        placeholder="Brazil, Europe"
        value={typed}
        onChange={setTyped}
        disabled={pending}
        onAdd={() => {
          setAdded(withTyped(added, typed));
          setTyped("");
        }}
      />
      {anyWanted && (
        <div className="flex flex-wrap items-center gap-x-3 gap-y-2">
          <span className="text-caption text-fg-muted">How much it matters</span>
          <Importance name={`${id}-importance`} legend="How much remote scope matters" value={importance} withOff={false} disabled={pending} onChange={setImportance} />
        </div>
      )}
      <PlaceChips
        label="Places you'd rather avoid"
        items={avoided.map((g) => ({ key: g.record.id, text: g.place, note: "avoid" }))}
        disabled={pending}
        onRemove={(key) => setAvoided(avoided.filter((g) => g.record.id !== key))}
      />
      <div className="flex flex-col gap-1.5">
        {anywhere && <Note>Anywhere means no geographic restriction: it doesn&apos;t rank remote roles up or down.</Note>}
        <IfUnknown>A listing that only says “Remote” doesn&apos;t say where, so it stays unresolved.</IfUnknown>
      </div>
    </EditorForm>
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

const POLICY_QUESTION = {
  eligibility: "Show roles when your eligibility is unclear?",
  pay: "Show roles that don't publish pay?",
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
  const text = POLICY[kind].find((o) => o.value === value)?.label ?? value;
  return (
    <PreferenceRow
      rowKey={rowKey}
      label={label}
      question={POLICY_QUESTION[kind]}
      now={text}
      actionLabel="Change"
      value={text}
      editor={(done) => <PolicyEditor label={label} kind={kind} value={value} update={update} done={done} />}
    />
  );
}

function PolicyEditor({ label, kind, value, update, done }: { label: string; kind: "eligibility" | "pay"; value: string; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const id = useId();
  const [choice, setChoice] = useState(value);
  return (
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        const show = choice === "show";
        commit([kind === "pay" ? { kind: "unknown_pay", show } : { kind: "unclear_eligibility", show }], [], done);
      }}
    >
      <ChoiceList name={`${id}-policy`} legend={label} options={POLICY[kind]} value={choice} onChange={setChoice} />
    </EditorForm>
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
  const text = [main && `${minimum ? "At least" : "Around"} ${payText(main)}`, ...records.filter((r) => r.applies_to).map(payText)].filter(Boolean).join("; ");
  return (
    <PreferenceRow
      rowKey={`pay-${bound}`}
      label={minimum ? "Minimum" : "Target"}
      question={minimum ? "What's the least you'd consider?" : "What pay are you aiming for?"}
      now={text}
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
      importance={records.length > 0 ? <LayerTag layer={(main ?? records[0])?.record.layer} /> : undefined}
      editor={(done) => <PayEditor bound={bound} records={records} update={update} done={done} />}
    />
  );
}

function PayEditor({ bound, records, update, done }: { bound: "minimum" | "target"; records: PayControl[]; update: UpdatePreferences; done: Done }) {
  const { pending, error: saveError, commit } = useCommit(update);
  const id = useId();
  const main = records.find((r) => !r.applies_to);
  const [removed, setRemoved] = useState<string[]>([]);
  const others = records.filter((r) => r.applies_to && !removed.includes(r.record.id));
  const [amount, setAmount] = useState(main ? String(main.amount) : "");
  const [currency, setCurrency] = useState(main?.currency ?? "");
  const [period, setPeriod] = useState<Period>(periodOf(main?.period));
  const [error, setError] = useState<string | null>(null);
  const minimum = bound === "minimum";
  return (
    <EditorForm
      pending={pending}
      error={error ?? saveError}
      onCancel={() => done()}
      extra={
        main && (
          <button type="button" disabled={pending} onClick={() => commit([], [main.record.id, ...removed], () => done("Removed."))} className={`${inlineActionClass} max-sm:min-h-11`}>
            Remove {bound}
          </button>
        )
      }
      onSubmit={() => {
        setError(null);
        // Only a figure for one arrangement removed, and nothing else entered.
        if (!main && !amount.trim() && removed.length > 0) return commit([], removed, done);
        const value = Number(amount.replace(/[,_\s]/g, ""));
        const code = currency.trim().toUpperCase();
        if (!amount.trim() || !Number.isFinite(value) || value <= 0) return setError("Enter the amount as a number, like 140000.");
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
          removed,
          done,
        );
      }}
    >
      <div className="grid grid-cols-[minmax(0,1fr)_96px] gap-2 sm:grid-cols-[minmax(0,1fr)_88px_112px]">
        <div className="min-w-0 max-sm:col-span-2">
          <label htmlFor={`${id}-amount`} className={labelClass}>
            Amount
          </label>
          <input id={`${id}-amount`} value={amount} onChange={(e) => setAmount(e.target.value)} inputMode="numeric" autoComplete="off" placeholder="140,000" className={inputClass} />
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
      <div className="flex flex-col gap-1.5">
        {minimum ? (
          <>
            <Note>Verified pay below it is left out.</Note>
            <IfUnknown>Pay that isn&apos;t published, or is in another currency or period (never converted), never counts as meeting it.</IfUnknown>
          </>
        ) : (
          <Note>Changes the order. Leaves nothing out.</Note>
        )}
      </div>
      {others.length > 0 && (
        <div>
          <p className={labelClass}>For one arrangement only</p>
          <ul className="divide-y divide-line-subtle border-y border-line-subtle text-[13px] text-fg-body">
            {others.map((o) => (
              <li key={o.record.id} className="flex min-h-10 items-center justify-between gap-3 py-1.5">
                <span className="min-w-0 nr-tnum">{payText(o)}</span>
                <button type="button" disabled={pending} onClick={() => setRemoved([...removed, o.record.id])} className={`${inlineActionClass} max-sm:min-h-11`}>
                  Remove <span className="sr-only">{payText(o)}</span>
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}
    </EditorForm>
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

const SCOPE: Record<string, { title: string; question: string }> = {
  team: { title: "Team", question: "What size of team suits you?" },
  company_size: { title: "Company size", question: "What size of company suits you?" },
  stage: { title: "Stage", question: "Which company stages suit you?" },
};

function companyText(on: CompanyControl[]): string {
  return on.map((i) => `${KINDS[i.value]?.label ?? i.value} · ${IMPORTANCE_TEXT[i.importance] ?? i.importance}`).join("; ");
}

function CompanyScope({ scope, items, update }: { scope: string; items: CompanyControl[]; update: UpdatePreferences }) {
  const on = items.filter((i) => i.importance !== "off");
  const { title, question } = SCOPE[scope] ?? { title: scope, question: scope };
  return (
    <PreferenceRow
      rowKey={`company-${scope}`}
      label={title}
      question={question}
      now={companyText(on)}
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
      editor={(done) => <CompanyEditor items={items} update={update} done={done} />}
    />
  );
}

function stanceOf(importance: string): "require" | "avoid" | "want" {
  return importance === "must_have" ? "require" : importance === "avoid" ? "avoid" : "want";
}

function CompanyEditor({ items, update, done }: { items: CompanyControl[]; update: UpdatePreferences; done: Done }) {
  const { pending, error, commit } = useCommit(update);
  const id = useId();
  const [draft, setDraft] = useState<Record<string, string>>(() => Object.fromEntries(items.map((i) => [i.value, i.importance])));
  const anyRequired = Object.values(draft).includes("must_have");
  return (
    <EditorForm
      pending={pending}
      error={error}
      onCancel={() => done()}
      onSubmit={() => {
        const set: PreferenceInput[] = [];
        const remove: string[] = [];
        for (const i of items) {
          const next = draft[i.value] ?? i.importance;
          if (next === i.importance) continue;
          if (next === "off") {
            if (i.record) remove.push(i.record.id);
          } else set.push({ kind: "company", company: i.value, stance: stanceOf(next) });
        }
        if (set.length === 0 && remove.length === 0) return done();
        commit(set, remove, done);
      }}
    >
      <ul className="divide-y divide-line-subtle border-y border-line-subtle">
        {items.map((i) => {
          const kind = KINDS[i.value] ?? { label: i.value, hint: "" };
          const labelId = `${id}-${i.value}`;
          return (
            <li key={i.value}>
              <div role="group" aria-labelledby={labelId} className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 py-3">
                <div className="min-w-0">
                  <p id={labelId} className="text-[13.5px] font-medium text-fg">
                    {kind.label}
                  </p>
                  {kind.hint && <p className="text-caption text-fg-muted">{kind.hint}</p>}
                </div>
                <Importance
                  name={`${labelId}-importance`}
                  legend="How much it matters"
                  value={draft[i.value] ?? i.importance}
                  withAvoid
                  disabled={pending}
                  onChange={(value) => setDraft({ ...draft, [i.value]: value })}
                />
              </div>
            </li>
          );
        })}
      </ul>
      <div className="flex flex-col gap-1.5">
        <Note>Must have leaves out roles that say otherwise. Nice to have and Avoid only change the order.</Note>
        {anyRequired && <IfUnknown>If a posting doesn&apos;t say, a must have stays unresolved and is never a Strong fit.</IfUnknown>}
      </div>
    </EditorForm>
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
