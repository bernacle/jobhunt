import type { ActionResult } from "@/app/actions";
import type { LearnedView, PreferenceUpdateResult, PreferenceView, TasteView } from "@/lib/api-types";
import { STANCE_LABEL, ago } from "@/lib/format";

import { type ClarifyAction, ClarifyPreference } from "./clarify";
import { RemovePreference } from "./preferences";

/*
 * Explicit and learned, kept apart. What the person told Narrow is a solid
 * square in primary ink: it always applies and always wins. What Narrow
 * learned from their decisions is a hollow square in secondary ink: a
 * tendency that only affects ranking, never a hidden filter. The two
 * columns and the words say so too, not only the marks.
 */

type Remove = (id: string) => Promise<ActionResult<PreferenceUpdateResult>>;

// Stated categories and learned dimensions, in the same groups.
const GROUPS: { title: string; stated: string[]; learned: string[] }[] = [
  { title: "Roles", stated: ["role"], learned: ["role", "seniority", "technology"] },
  { title: "Pay", stated: ["compensation"], learned: ["compensation"] },
  { title: "Location and work", stated: ["location"], learned: [] },
  { title: "Company and team", stated: ["company"], learned: ["company_trait", "company"] },
  { title: "Domains and products", stated: ["domain"], learned: ["domain", "product"] },
  { title: "Way of working", stated: ["work_style"], learned: ["work_style"] },
];

const STANCE_ORDER = ["required", "wanted", "acceptable", "unwanted"];

function learnedSentence(l: LearnedView): string {
  return `${l.direction === "prefer" ? "You tend to go for" : "You tend to pass on"} ${l.value}`;
}

function Square({ learned }: { learned: boolean }) {
  return (
    <span
      aria-hidden="true"
      className={`mt-[0.45em] size-[7px] shrink-0 rounded-[1px] ${learned ? "border border-fg-secondary" : "bg-fg"}`}
    />
  );
}

export function ExplicitPreference({ p, remove, clarify }: { p: PreferenceView; remove: Remove; clarify?: ClarifyAction }) {
  return (
    <li className="flex gap-3 border-b border-line-subtle py-3">
      <Square learned={false} />
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline justify-between gap-3">
          <p className="text-[14px] font-medium text-fg nr-tnum">
            {STANCE_LABEL[p.stance] ?? p.stance}: {p.value}
          </p>
          <RemovePreference id={p.id} label={`${p.stance} ${p.value}`} remove={remove} />
        </div>
        <p className="mt-0.5 text-caption text-fg-muted">
          {p.snippet ? (
            <>
              From your words: <q>{p.snippet}</q>
            </>
          ) : (
            "Set by you"
          )}
        </p>
        {p.clarify && clarify ? (
          <ClarifyPreference p={p} clarify={clarify} />
        ) : (
          p.certainty === "uncertain" && (
            <p className="mt-1 text-[13px] text-fg-secondary">
              <span className="nr-inferred">Narrow isn&apos;t sure it read this right</span>
              {p.note && <>: {p.note}</>}
            </p>
          )
        )}
      </div>
    </li>
  );
}

function Pattern({ l }: { l: LearnedView }) {
  return (
    <li className="flex gap-3 border-b border-line-subtle py-3">
      <Square learned />
      <div className="min-w-0 flex-1">
        <p className="text-[14px] text-fg-secondary">{learnedSentence(l)}</p>
        <p className="mt-0.5 font-mono text-mono-s text-fg-muted">
          {l.confidence && <>{l.confidence} · </>}
          {l.basis}
          {l.opportunities > 1 && <> across {l.opportunities} jobs</>} · last {ago(l.last_reinforced)}
        </p>
        {l.stated && (
          <p className="mt-1 text-[13px] text-fg-secondary">
            You said “{l.stated}”{l.agrees ? ", and your decisions agree." : "; your decisions lean the other way, and what you said wins."}
          </p>
        )}
        <details className="group mt-1 text-[13px]">
          <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 font-medium text-fg-secondary hover:text-fg max-sm:min-h-11">
            <span aria-hidden="true" className="text-fg-muted transition-transform duration-[120ms] group-open:rotate-90">
              ›
            </span>
            Why Narrow thinks so
          </summary>
          <ul className="mt-1.5 space-y-1 border-l border-line-subtle pl-3 text-fg-body">
            {l.support.map((e) => (
              <li key={`${e.opportunity}-${e.at}-${e.read_as}`}>
                {e.summary}
                {e.reason && (
                  <span className="text-fg-secondary">
                    {" "}
                    — you wrote <q>{e.reason}</q>
                  </span>
                )}
              </li>
            ))}
            {l.against.map((e) => (
              <li key={`against-${e.opportunity}-${e.at}-${e.read_as}`} className="text-fg-muted">
                Against: {e.summary}
              </li>
            ))}
          </ul>
        </details>
      </div>
    </li>
  );
}

function Empty({ children }: { children: string }) {
  return <p className="border-b border-line-subtle py-3 text-row text-fg-muted">{children}</p>;
}

/**
 * Preferences by group: what the person told Narrow beside what it
 * learned, so a tendency can never pass for a requirement.
 */
export function TasteTable({ taste, remove, clarify }: { taste: TasteView; remove: Remove; clarify?: ClarifyAction }) {
  const stated = (categories: string[]) =>
    taste.stated
      .filter((p) => categories.includes(p.category))
      .sort((a, b) => STANCE_ORDER.indexOf(a.stance) - STANCE_ORDER.indexOf(b.stance));
  const known = GROUPS.flatMap((g) => g.learned);
  const knownStated = GROUPS.flatMap((g) => g.stated);
  const groups = [
    ...GROUPS.map((g) => ({ title: g.title, told: stated(g.stated), learned: taste.learned.filter((l) => g.learned.includes(l.dimension)) })),
    {
      title: "Other",
      told: taste.stated.filter((p) => !knownStated.includes(p.category)),
      learned: taste.learned.filter((l) => !known.includes(l.dimension)),
    },
  ].filter((g) => g.title !== "Other" || g.told.length + g.learned.length > 0);
  const column = "text-left align-top font-normal md:pb-3";
  return (
    <table className="w-full table-fixed border-collapse max-md:block">
      <thead className="max-md:hidden">
        <tr>
          <td className="w-[130px]" />
          <th scope="col" className={`${column} pr-8`}>
            <span className="block text-title-m">You told us</span>
            <span className="mt-1 block text-[13px] text-fg-muted">Always applied, and always wins.</span>
          </th>
          <th scope="col" className={column}>
            <span className="block text-title-m text-fg-secondary">We&apos;ve learned</span>
            <span className="mt-1 block text-[13px] text-fg-muted">Tendencies from your decisions. Ranking only.</span>
          </th>
        </tr>
      </thead>
      <tbody className="max-md:block">
        {groups.map((g) => (
          <tr key={g.title} className="border-t border-line-subtle max-md:block max-md:pt-1 max-md:pb-4">
            <th scope="row" className="w-[130px] pt-[15px] text-left align-top text-label text-fg-muted max-md:block max-md:w-auto max-md:pb-1">
              {g.title}
            </th>
            <td className="align-top md:pr-8 max-md:block">
              <p className="pt-2 text-[11px] font-medium text-fg-muted md:hidden">You told us</p>
              {g.told.length > 0 ? (
                <ul role="list">
                  {g.told.map((p) => (
                    <ExplicitPreference key={p.id} p={p} remove={remove} clarify={clarify} />
                  ))}
                </ul>
              ) : (
                <Empty>Nothing stated</Empty>
              )}
            </td>
            <td className="align-top max-md:block">
              <p className="pt-2 text-[11px] font-medium text-fg-muted md:hidden">We&apos;ve learned</p>
              {g.learned.length > 0 ? (
                <ul role="list">
                  {g.learned.map((l) => (
                    <Pattern key={l.key} l={l} />
                  ))}
                </ul>
              ) : (
                <Empty>Nothing learned yet</Empty>
              )}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}

/** Patterns that exist but aren't used, and reasons nothing was read from. */
export function NotInUse({ taste }: { taste: TasteView }) {
  const lists: { title: string; items: LearnedView[] }[] = [
    { title: "Contradictory, so not used", items: taste.contradictory },
    { title: "Covered by what you said", items: taste.covered_by_stated },
  ];
  const any = lists.some((l) => l.items.length > 0) || taste.emerging.length > 0 || taste.unread_reasons.length > 0;
  if (!any) return null;
  return (
    <div className="mt-6 space-y-2 text-[13px]">
      {lists
        .filter((l) => l.items.length > 0)
        .map((l) => (
          <details key={l.title} className="group">
            <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 text-fg-secondary hover:text-fg max-sm:min-h-11">
              <span aria-hidden="true" className="text-fg-muted transition-transform duration-[120ms] group-open:rotate-90">
                ›
              </span>
              {l.title} ({l.items.length})
            </summary>
            <ul role="list" className="mt-1">
              {l.items.map((item) => (
                <Pattern key={item.key} l={item} />
              ))}
            </ul>
          </details>
        ))}
      {taste.emerging.length > 0 && (
        <p className="text-fg-muted">
          {taste.emerging.length} more {taste.emerging.length === 1 ? "pattern has" : "patterns have"} too little evidence to use yet.
        </p>
      )}
      {taste.unread_reasons.length > 0 && (
        <details className="group">
          <summary className="inline-flex cursor-pointer list-none items-center gap-1.5 text-fg-secondary hover:text-fg max-sm:min-h-11">
            <span aria-hidden="true" className="text-fg-muted transition-transform duration-[120ms] group-open:rotate-90">
              ›
            </span>
            Reasons kept as written ({taste.unread_reasons.length})
          </summary>
          <ul className="mt-2 space-y-1 text-fg-body">
            {taste.unread_reasons.map((u) => (
              <li key={`${u.opportunity}-${u.at}`}>
                <q>{u.reason}</q>{" "}
                <span className="text-fg-muted">
                  — {u.title} at {u.company}
                </span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
