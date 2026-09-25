import type { LearnedView, TasteView } from "@/lib/api-types";
import { ago } from "@/lib/format";

function sentence(l: LearnedView): string {
  const lead = l.direction === "prefer" ? "You tend to go for" : "You tend to pass on";
  return `${lead} ${l.value}`;
}

function Pattern({ l }: { l: LearnedView }) {
  return (
    <li className="py-3">
      <p>
        <span className="font-medium">{sentence(l)}</span> <span className="text-muted">({l.dimension})</span>
      </p>
      <p className="text-sm text-muted">
        {l.confidence && <>{l.confidence} · </>}
        {l.basis}
        {l.opportunities > 1 && <> across {l.opportunities} jobs</>} · last {ago(l.last_reinforced)}
      </p>
      {l.stated && (
        <p className="text-sm text-muted">
          You said “{l.stated}”{l.agrees ? ", and your feedback agrees." : "; your feedback leans the other way, and what you said wins."}
        </p>
      )}
      <details className="mt-1 text-sm">
        <summary className="cursor-pointer text-muted underline">Why JobHunt thinks so</summary>
        <ul className="mt-2 space-y-1 border-l border-line pl-3">
          {l.support.map((e) => (
            <li key={`${e.opportunity}-${e.at}-${e.read_as}`}>
              {e.summary}
              {e.reason && (
                <span className="text-muted">
                  {" "}
                  — you wrote <q>{e.reason}</q>
                </span>
              )}
            </li>
          ))}
          {l.against.map((e) => (
            <li key={`against-${e.opportunity}-${e.at}-${e.read_as}`} className="text-muted">
              Against: {e.summary}
            </li>
          ))}
        </ul>
      </details>
    </li>
  );
}

/**
 * What JobHunt concluded from the person's feedback. Always labeled as
 * learned, apart from what they said, and always with its evidence.
 */
export function LearnedTaste({ taste }: { taste: TasteView }) {
  const nothing = taste.learned.length === 0;
  return (
    <div className="rounded-xl border border-dashed border-line-strong bg-learned p-5">
      <p className="text-xs font-semibold uppercase tracking-wider text-muted">Learned, not stated</p>
      <p className="mt-1 text-sm text-muted">
        From {taste.feedback_events} {taste.feedback_events === 1 ? "action" : "actions"} on {taste.opportunities}{" "}
        {taste.opportunities === 1 ? "job" : "jobs"}. What you tell JobHunt always wins over these.
      </p>
      {nothing ? (
        <p className="mt-4 text-sm">
          Nothing yet. Patterns appear after a reason in your own words, or after several jobs you treated the same way.
        </p>
      ) : (
        <ul className="mt-2 divide-y divide-line">
          {taste.learned.map((l) => (
            <Pattern key={l.key} l={l} />
          ))}
        </ul>
      )}
      {taste.contradictory.length > 0 && (
        <details className="mt-3 text-sm">
          <summary className="cursor-pointer text-muted">Contradictory, so not used ({taste.contradictory.length})</summary>
          <ul className="divide-y divide-line">
            {taste.contradictory.map((l) => (
              <Pattern key={l.key} l={l} />
            ))}
          </ul>
        </details>
      )}
      {taste.covered_by_stated.length > 0 && (
        <details className="mt-3 text-sm">
          <summary className="cursor-pointer text-muted">Covered by what you said ({taste.covered_by_stated.length})</summary>
          <ul className="divide-y divide-line">
            {taste.covered_by_stated.map((l) => (
              <Pattern key={l.key} l={l} />
            ))}
          </ul>
        </details>
      )}
      {taste.emerging.length > 0 && (
        <p className="mt-3 text-sm text-muted">
          {taste.emerging.length} more {taste.emerging.length === 1 ? "pattern has" : "patterns have"} too little evidence to use yet.
        </p>
      )}
      {taste.unread_reasons.length > 0 && (
        <details className="mt-3 text-sm">
          <summary className="cursor-pointer text-muted">Reasons kept as written ({taste.unread_reasons.length})</summary>
          <ul className="mt-2 space-y-1">
            {taste.unread_reasons.map((u) => (
              <li key={`${u.opportunity}-${u.at}`}>
                <q>{u.reason}</q> <span className="text-muted">— {u.title} at {u.company}</span>
              </li>
            ))}
          </ul>
        </details>
      )}
    </div>
  );
}
