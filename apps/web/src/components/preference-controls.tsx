"use client";

import { type ReactNode, useId, useOptimistic, useState, useTransition } from "react";

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

import { Button, inlineActionClass, inputClass, labelClass, selectClass } from "./ui";

/*
 * Configuring Narrow's judgment, not filtering a job board. Each control
 * reads the same preference records a sentence in the person's words
 * writes (the API derives the controls from them), and each change sends
 * the same structured inputs the CLI and AI assistants use. Nothing here
 * decides what a value means: the API does, and says which layer each
 * setting is in.
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

/** What each layer does, once, at the top of the page. */
export function LayerLegend() {
  const items: { layer: Layer; text: ReactNode }[] = [
    {
      layer: "requirement",
      text: "A job that states the opposite is left out. If the posting doesn't say, it stays unresolved and is never a Strong fit.",
    },
    { layer: "preference", text: "Changes the order. Never leaves a job out." },
    { layer: "learned", text: "From your decisions. Ranking only, and what you say always wins." },
  ];
  return (
    <dl aria-label="How Narrow uses what you tell it" className="grid gap-x-8 gap-y-2.5 border-y border-line-subtle py-4 md:grid-cols-3">
      {items.map((i) => (
        <div key={i.layer} className="min-w-0">
          <dt className="flex items-center gap-2 text-[13px] font-semibold text-fg">
            <LayerMark layer={i.layer} />
            {LAYER_TEXT[i.layer]}
          </dt>
          <dd className="mt-0.5 pl-[15px] text-caption text-fg-muted">{i.text}</dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * Form fields that start from what is stored and start over when it
 * changes elsewhere (a sentence in the person's words, another device),
 * keeping everything else (the last outcome) as it is.
 */
function useStored<T>(stored: T, key: string): [T, (value: T) => void] {
  const [value, setValue] = useState(stored);
  const [seen, setSeen] = useState(key);
  if (seen !== key) {
    setSeen(key);
    setValue(stored);
  }
  return [value, setValue];
}

/** Runs one change and keeps its outcome for the person. */
function useSave(update: UpdatePreferences) {
  const [pending, startTransition] = useTransition();
  const [message, setMessage] = useState<{ ok: boolean; text: string } | null>(null);
  /** `before` runs inside the transition (an optimistic choice). */
  const save = (set: PreferenceInput[], remove: string[] = [], before?: () => void) =>
    startTransition(async () => {
      before?.();
      setMessage(null);
      const r = await update(set, remove);
      if (r.ok) {
        setMessage({ ok: true, text: r.data.unchanged ? "Already in effect." : "Saved." });
      } else {
        setMessage({ ok: false, text: `${r.title}. ${r.message}` });
      }
    });
  return { pending, message, save };
}

function Outcome({ message }: { message: { ok: boolean; text: string } | null }) {
  return (
    <span role="status" aria-live="polite" className={`text-caption ${message?.ok === false ? "text-danger" : "text-fg-muted"}`}>
      {message?.text}
    </span>
  );
}

/** One setting: a label, what it's for, its layer now, and the control. */
function Row({ title, help, layer, children, id }: { title: string; help?: ReactNode; layer?: string | null; children: ReactNode; id: string }) {
  return (
    <div role="group" aria-labelledby={id} className="grid gap-x-8 gap-y-2 border-b border-line-subtle py-5 md:grid-cols-[220px_minmax(0,1fr)]">
      <div className="min-w-0">
        <p id={id} className="text-[14px] font-medium text-fg">
          {title}
        </p>
        {help && <p className="mt-0.5 text-caption text-fg-muted">{help}</p>}
        {layer && (
          <p className="mt-1.5">
            <LayerTag layer={layer} />
          </p>
        )}
      </div>
      <div className="min-w-0">{children}</div>
    </div>
  );
}

interface Option {
  value: string;
  label: string;
  hint?: string;
}

/**
 * A choice among a few answers, as bordered options: a grid on wide
 * screens, stacked 44px rows on a phone. The row around it names it.
 */
function Choices({
  name,
  options,
  value,
  onChange,
  disabled,
  columns = "sm:grid-cols-2 lg:grid-cols-3",
}: {
  name: string;
  options: Option[];
  value: string | null;
  onChange: (value: string) => void;
  disabled?: boolean;
  columns?: string;
}) {
  return (
    <fieldset disabled={disabled}>
      <div className={`grid gap-2 ${columns}`}>
        {options.map((o) => (
          <label
            key={o.value}
            className={
              "flex min-h-11 cursor-pointer gap-2.5 rounded-md border border-line px-3 py-2.5 transition-colors duration-[120ms] " +
              "hover:border-line-strong has-checked:border-fg has-checked:bg-raised has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) " +
              "has-disabled:cursor-default"
            }
          >
            <input
              type="radio"
              name={name}
              value={o.value}
              checked={value === o.value}
              onChange={() => onChange(o.value)}
              className="mt-[3px] size-3.5 shrink-0 accent-fg"
            />
            <span className="min-w-0">
              <span className="block text-[13.5px] font-medium text-fg">{o.label}</span>
              {o.hint && <span className="mt-0.5 block text-caption text-fg-muted">{o.hint}</span>}
            </span>
          </label>
        ))}
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
    <fieldset disabled={disabled} className="min-w-0">
      <legend className="sr-only">{legend}</legend>
      <div className="inline-flex max-w-full flex-wrap rounded-md border border-line p-0.5">
        {options.map((o) => (
          <label
            key={o.value}
            className={
              "flex min-h-8 cursor-pointer items-center rounded-[5px] px-2.5 text-[12.5px] font-medium text-fg-secondary transition-colors duration-[120ms] " +
              "hover:text-fg has-checked:bg-selected has-checked:text-fg has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) max-sm:min-h-11"
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

function Note({ p }: { p?: PreferenceView | null }) {
  if (!p) return null;
  return (
    <p className="mt-2 text-caption text-fg-muted">
      {p.snippet ? (
        <>
          From your words: <q>{p.snippet}</q>
        </>
      ) : (
        "Set by you"
      )}
      {p.clarify && <span className="text-fg-secondary"> · Needs your answer (see In your words)</span>}
    </p>
  );
}

// ---------------------------------------------------------------------------
// Work

const SETUPS: (Option & { layer: Layer | null })[] = [
  { value: "remote_only", label: "Remote only", hint: "Hybrid and on-site roles are left out.", layer: "requirement" },
  { value: "prefer_remote", label: "Prefer remote", hint: "Remote roles rank higher. Nothing is left out.", layer: "preference" },
  { value: "hybrid_okay", label: "Hybrid okay", hint: "Remote or hybrid. On-site roles are left out.", layer: "requirement" },
  { value: "onsite_okay", label: "On-site okay", hint: "Any setup works for you.", layer: "preference" },
  { value: "no_preference", label: "No preference", hint: "Work setup doesn't count either way.", layer: null },
];

const RELOCATION: Option[] = [
  { value: "not_willing", label: "Not willing to relocate", hint: "Roles that need you somewhere else are left out." },
  { value: "open", label: "Open to relocation", hint: "Those roles stay, with the move as a condition." },
  { value: "only_selected", label: "Only to some places", hint: "Name the countries or regions." },
];

function WorkSetup({ work, update }: { work: PreferenceControls["work"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const [shown, setShown] = useOptimistic(work.setup);
  const current = SETUPS.find((s) => s.value === shown);
  return (
    <Row id={id} title="Work setup" help="Remote, hybrid or on-site. Not the same as where you may legally work." layer={work.setup_layer === "none" ? null : work.setup_layer}>
      {work.setup === "custom" && (
        <p className="mb-3 text-[13px] text-fg-secondary">
          From your words: {work.custom}. Choose one below to replace it.
        </p>
      )}
      <Choices
        name={`${id}-setup`}
        options={SETUPS}
        value={current ? current.value : null}
        disabled={pending}
        onChange={(setup) => save([{ kind: "work_setup", setup: setup as "remote_only" }], [], () => setShown(setup))}
      />
      <div className="mt-2 flex flex-wrap items-baseline gap-x-3">
        <Outcome message={message} />
      </div>
      {work.setup_records.find((r) => r.origin === "statement") && <Note p={work.setup_records.find((r) => r.origin === "statement")} />}
    </Row>
  );
}

function Relocation({ work, update }: { work: PreferenceControls["work"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const stored = `${work.relocation}:${work.relocation_only_to.join(",")}`;
  const [choice, setChoice] = useStored<string | null>(work.relocation === "unset" ? null : work.relocation, stored);
  const [places, setPlaces] = useStored(work.relocation_only_to.join(", "), stored);
  return (
    <Row id={id} title="Relocation" help="Whether you'd move for a role. Separate from your work setup." layer={work.relocation === "unset" ? null : "requirement"}>
      <Choices
        name={`${id}-relocation`}
        options={RELOCATION}
        value={choice}
        disabled={pending}
        onChange={(value) => {
          setChoice(value);
          if (value === "not_willing") save([{ kind: "relocation", willing: false, only_to: [] }]);
          if (value === "open") save([{ kind: "relocation", willing: true, only_to: [] }]);
        }}
      />
      {choice === "only_selected" && (
        <form
          className="mt-3 flex flex-wrap items-end gap-2"
          onSubmit={(e) => {
            e.preventDefault();
            const list = places
              .split(",")
              .map((p) => p.trim())
              .filter(Boolean);
            if (list.length > 0) save([{ kind: "relocation", willing: true, only_to: list }]);
          }}
        >
          <div className="min-w-0 flex-1 basis-60">
            <label htmlFor={`${id}-places`} className={labelClass}>
              Countries or regions
            </label>
            <input id={`${id}-places`} value={places} onChange={(e) => setPlaces(e.target.value)} placeholder="Portugal, Spain" required className={inputClass} />
          </div>
          <Button type="submit" disabled={pending} loading={pending} className="max-sm:h-11">
            Save places
          </Button>
        </form>
      )}
      <div className="mt-2">
        <Outcome message={message} />
      </div>
      <Note p={work.relocation_record} />
    </Row>
  );
}

// ---------------------------------------------------------------------------
// Location

function Home({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const [place, setPlace] = useStored(location.home ?? "", location.home ?? "");
  return (
    <Row id={id} title="Where you live" help="Your home country. Remote scopes are matched against it." layer={location.home ? "requirement" : null}>
      <form
        className="flex flex-wrap items-end gap-2"
        onSubmit={(e) => {
          e.preventDefault();
          if (place.trim()) save([{ kind: "location", place: place.trim() }]);
        }}
      >
        <div className="min-w-0 flex-1 basis-60">
          <label htmlFor={`${id}-home`} className="sr-only">
            Where you live
          </label>
          <input id={`${id}-home`} value={place} onChange={(e) => setPlace(e.target.value)} placeholder="São Paulo, Brazil" required className={inputClass} />
        </div>
        <Button type="submit" disabled={pending} loading={pending} className="max-sm:h-11">
          Save
        </Button>
        <Outcome message={message} />
      </form>
      <div className="mt-2 space-y-1 text-caption text-fg-muted">
        {location.home && location.home_basis === "resume" && <p>From your resume. Save it to make it yours.</p>}
        {location.home && !location.home_country && <p className="text-fg-secondary">Narrow doesn&apos;t recognize this place, so remote scopes can&apos;t be matched to it.</p>}
        {location.remote_open_to_you.length > 0 && (
          <p>
            Remote roles open to {listText(location.remote_open_to_you)} include you. A listing that only says “Remote” doesn&apos;t say where, so
            it stays unresolved.
          </p>
        )}
      </div>
    </Row>
  );
}

function listText(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} or ${items[items.length - 1]}`;
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
        Remove<span className="sr-only"> {place.place}</span>
      </button>
    </li>
  );
}

function AddPlace({ label, placeholder, onAdd, pending }: { label: string; placeholder: string; onAdd: (place: string) => void; pending: boolean }) {
  const id = useId();
  const [value, setValue] = useState("");
  return (
    <form
      className="mt-3 flex flex-wrap items-end gap-2"
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

function Authorization({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  return (
    <Row
      id={id}
      title="Where you may legally work"
      help="Countries or regions where you can work without sponsorship. Separate from where you live."
      layer={location.authorized_in.length > 0 ? "requirement" : null}
    >
      {location.authorized_in.length > 0 ? (
        <ul className="flex flex-wrap gap-2">
          {location.authorized_in.map((a) => (
            <PlaceChip key={a.record.id} place={a} pending={pending} onRemove={() => save([], [a.record.id])} />
          ))}
        </ul>
      ) : (
        <p className="text-[13px] text-fg-muted">Not said. Where a posting requires authorization, it stays unresolved.</p>
      )}
      <AddPlace label="Add a country or region" placeholder="Brazil, the EU" pending={pending} onAdd={(place) => save([{ kind: "authorized_in", place }])} />
      <div className="mt-2">
        <Outcome message={message} />
      </div>
    </Row>
  );
}

const SCOPES = [
  { code: "worldwide", value: "Worldwide", label: "Anywhere (no restriction)" },
  { code: "americas", value: "Americas", label: "The Americas" },
  { code: "latam", value: "Latin America", label: "Latin America (LATAM)" },
];

function RemoteGeography({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const wanted = location.remote_geography.filter((g) => g.record.stance !== "unwanted");
  const avoided = location.remote_geography.filter((g) => g.record.stance === "unwanted");
  const importance = wanted.some((g) => g.record.stance === "required") ? "must_have" : "nice_to_have";
  const stance = importance === "must_have" ? "require" : "want";
  const presetOf = (code: string) => wanted.find((g) => g.code === code);
  const others = wanted.filter((g) => !SCOPES.some((s) => s.code === g.code));
  // Anywhere restricts nothing and ranks nothing up or down; a required list
  // that includes it restricts nothing either.
  const anywhere = Boolean(presetOf("worldwide"));
  const restricting = importance === "must_have" && anywhere ? [] : wanted.filter((g) => g.code !== "worldwide");
  return (
    <Row
      id={id}
      title="Remote roles open to"
      help="Where a remote role's published scope should reach. Leave empty to rely on where you live."
      layer={restricting.length === 0 ? null : importance === "must_have" ? "requirement" : "preference"}
    >
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
                  "hover:border-line-strong has-checked:border-fg has-checked:bg-raised has-focus-visible:outline-[1.5px] has-focus-visible:outline-(--nr-focus) max-sm:min-h-11"
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
      {anywhere && (
        <p className="mt-2 text-caption text-fg-muted">
          Anywhere means no geographic restriction: it doesn&apos;t rank remote roles up or down. Eligibility still depends on where you
          live.
        </p>
      )}
      {others.length > 0 && (
        <ul aria-label="Other places" className="mt-3 flex flex-wrap gap-2">
          {others.map((g) => (
            <PlaceChip key={g.record.id} place={g} pending={pending} onRemove={() => save([], [g.record.id])} />
          ))}
        </ul>
      )}
      <AddPlace
        label="Add a country or region"
        placeholder="Brazil, Europe"
        pending={pending}
        onAdd={(region) => save([{ kind: "region", region, stance }])}
      />
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
      <div className="mt-2">
        <Outcome message={message} />
      </div>
    </Row>
  );
}

function UnclearEligibility({ location, update }: { location: PreferenceControls["location"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const [shown, setShown] = useOptimistic(location.unclear_eligibility);
  return (
    <Row id={id} title="When eligibility is unclear" help="For example a listing that says “Remote” with no place." layer="requirement">
      <Choices
        name={`${id}-unclear`}
        columns="sm:grid-cols-2"
        options={[
          { value: "show", label: "Show me, marked unresolved", hint: "You decide once you know more." },
          { value: "hide", label: "Only when it's confirmed", hint: "Left out until Narrow can confirm it." },
        ]}
        value={shown}
        disabled={pending}
        onChange={(value) => save([{ kind: "unclear_eligibility", show: value === "show" }], [], () => setShown(value))}
      />
      <div className="mt-2">
        <Outcome message={message} />
      </div>
    </Row>
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

function PayRow({
  bound,
  records,
  update,
}: {
  bound: "minimum" | "target";
  records: PayControl[];
  update: UpdatePreferences;
}) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  // The figure for both arrangements is the one edited here.
  const main = records.find((r) => !r.applies_to);
  const others = records.filter((r) => r.applies_to);
  const stored = main ? `${main.record.id}:${main.amount}:${main.currency}:${main.period}` : "";
  const [amount, setAmount] = useStored(main ? String(main.amount) : "", stored);
  const [currency, setCurrency] = useStored(main?.currency ?? "", stored);
  const [period, setPeriod] = useStored<Period>(periodOf(main?.period), stored);
  const [error, setError] = useState<string | null>(null);
  const minimum = bound === "minimum";
  return (
    <Row
      id={id}
      title={minimum ? "Minimum" : "Target"}
      help={
        minimum
          ? "A floor. Verified pay below it is left out; pay that isn't published never counts as meeting it."
          : "What you're aiming for. It changes the order and leaves nothing out."
      }
      layer={records.length > 0 ? (minimum ? "requirement" : "preference") : null}
    >
      {main && (
        <p className="mb-3 text-[14px] font-medium text-fg nr-tnum">
          {minimum ? "At least " : "Around "}
          {payText(main)}
        </p>
      )}
      <form
        className="grid gap-2 sm:grid-cols-[minmax(0,1fr)_96px_132px_auto] sm:items-end"
        onSubmit={(e) => {
          e.preventDefault();
          setError(null);
          const value = Number(amount.replace(/[,_\s]/g, ""));
          const code = currency.trim().toUpperCase();
          if (!Number.isFinite(value) || value <= 0) return setError("Enter the amount as a number, like 140000.");
          if (!/^[A-Z]{3}$/.test(code)) return setError("Choose the currency: a three-letter code like USD or EUR. Narrow never assumes one.");
          save([
            {
              kind: "compensation",
              minimum: minimum ? value : null,
              target: minimum ? null : value,
              currency: code,
              period,
              applies_to: null,
            },
          ]);
        }}
      >
        <div className="min-w-0">
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
        <div className="flex items-center gap-3">
          <Button type="submit" disabled={pending} loading={pending} className="max-sm:h-11">
            {main ? "Update" : "Set"}
          </Button>
          {main && (
            <button type="button" disabled={pending} onClick={() => save([], [main.record.id])} className={`${inlineActionClass} max-sm:min-h-11`}>
              Remove<span className="sr-only"> {bound}</span>
            </button>
          )}
        </div>
      </form>
      <div className="mt-2">
        {error ? (
          <span role="alert" className="text-caption text-danger">
            {error}
          </span>
        ) : (
          <Outcome message={message} />
        )}
      </div>
      {others.length > 0 && (
        <ul className="mt-3 space-y-1 text-[13px] text-fg-body">
          {others.map((o) => (
            <li key={o.record.id} className="flex items-baseline justify-between gap-3">
              <span className="nr-tnum">{payText(o)}</span>
              <button type="button" disabled={pending} onClick={() => save([], [o.record.id])} className={inlineActionClass}>
                Remove<span className="sr-only"> {payText(o)}</span>
              </button>
            </li>
          ))}
        </ul>
      )}
      <Note p={main?.record} />
    </Row>
  );
}

function UnknownPay({ pay, update }: { pay: PreferenceControls["pay"]; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const [shown, setShown] = useOptimistic(pay.unknown_pay);
  return (
    <Row id={id} title="When pay isn't published" help="Or can't be compared with yours (another currency or period)." layer="requirement">
      <Choices
        name={`${id}-unknown`}
        columns="sm:grid-cols-2"
        options={[
          { value: "show", label: "Show them, marked unresolved", hint: "Unknown pay never counts as meeting your minimum." },
          { value: "hide", label: "Hide them", hint: "Only roles that publish comparable pay." },
        ]}
        value={shown}
        disabled={pending}
        onChange={(value) => save([{ kind: "unknown_pay", show: value === "show" }], [], () => setShown(value))}
      />
      <div className="mt-2">
        <Outcome message={message} />
      </div>
    </Row>
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

function CompanyKind({ item, update }: { item: CompanyControl; update: UpdatePreferences }) {
  const { pending, message, save } = useSave(update);
  const id = useId();
  const kind = KINDS[item.value] ?? { label: item.value, hint: "" };
  const [shown, setShown] = useOptimistic(item.importance);
  return (
    <li>
      <div role="group" aria-labelledby={id} className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2 py-3">
      <div className="min-w-0">
        <p id={id} className="text-[13.5px] font-medium text-fg">
          {kind.label}
        </p>
        {kind.hint && <p className="text-caption text-fg-muted">{kind.hint}</p>}
        <p className="mt-0.5 flex flex-wrap items-center gap-x-3">
          <LayerTag layer={item.layer} />
          <Outcome message={message} />
        </p>
      </div>
      <Importance
        name={`${id}-importance`}
        legend="How much it matters"
        value={shown}
        withAvoid
        disabled={pending}
        onChange={(value) => {
          if (value === "off") {
            if (item.record) save([], [item.record.id], () => setShown(value));
            return;
          }
          const stance = value === "must_have" ? "require" : value === "avoid" ? "avoid" : "want";
          save([{ kind: "company", company: item.value, stance }], [], () => setShown(value));
        }}
      />
      </div>
    </li>
  );
}

function CompanyAndTeam({ company, update }: { company: PreferenceControls["company"]; update: UpdatePreferences }) {
  const scopes = ["team", "company_size", "stage"];
  return (
    <div className="grid gap-x-8 md:grid-cols-[220px_minmax(0,1fr)]">
      <p className="pt-3 text-caption text-fg-muted max-md:hidden">
        A company&apos;s size never stands in for your team&apos;s. Must have: a posting that says otherwise is left out, and one that doesn&apos;t say
        stays unresolved.
      </p>
      <div className="min-w-0">
        {scopes.map((scope) => (
          <div key={scope} className="border-b border-line-subtle pb-2 pt-4 first:pt-2">
            <h3 className="text-label text-fg-muted">{SCOPE_TITLE[scope]}</h3>
            <ul className="divide-y divide-line-subtle">
              {company.items
                .filter((i) => i.scope === scope)
                .map((i) => (
                  <CompanyKind key={i.value} item={i} update={update} />
                ))}
            </ul>
          </div>
        ))}
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// The sections

export function WorkControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <div className="border-t border-line-subtle">
      <WorkSetup work={controls.work} update={update} />
      <Relocation work={controls.work} update={update} />
    </div>
  );
}

export function LocationControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <div className="border-t border-line-subtle">
      <Home location={controls.location} update={update} />
      <Authorization location={controls.location} update={update} />
      <RemoteGeography location={controls.location} update={update} />
      <UnclearEligibility location={controls.location} update={update} />
    </div>
  );
}

export function PayControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <div className="border-t border-line-subtle">
      <PayRow bound="minimum" records={controls.pay.minimum} update={update} />
      <PayRow bound="target" records={controls.pay.target} update={update} />
      <UnknownPay pay={controls.pay} update={update} />
    </div>
  );
}

export function CompanyControls({ controls, update }: { controls: PreferenceControls; update: UpdatePreferences }) {
  return (
    <div className="border-t border-line-subtle">
      <CompanyAndTeam company={controls.company} update={update} />
    </div>
  );
}

