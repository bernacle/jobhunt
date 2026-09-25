import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { removePreference, setPreference, tellPreferences } from "@/app/actions";
import { LearnedTaste } from "@/components/learned-taste";
import { PreciseForm, RemovePreference, StatementForm } from "@/components/preferences";
import { PageHeader, Section } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import type { PreferenceView } from "@/lib/api-types";
import { STANCE_LABEL } from "@/lib/format";

export const metadata: Metadata = { title: "Preferences" };

const GROUPS: { category: string; title: string }[] = [
  { category: "role", title: "Roles" },
  { category: "compensation", title: "Pay" },
  { category: "location", title: "Location and work" },
  { category: "company", title: "Companies and teams" },
  { category: "domain", title: "Domains and products" },
  { category: "work_style", title: "How you like to work" },
];

const STANCE_ORDER = ["required", "wanted", "acceptable", "unwanted"];

export default async function PreferencesPage() {
  const taste = await loadOrNoProfile(() => api.taste());
  if (taste === "no_profile") redirect("/welcome");
  const byCategory = (category: string): PreferenceView[] =>
    taste.stated
      .filter((p) => p.category === category)
      .sort((a, b) => STANCE_ORDER.indexOf(a.stance) - STANCE_ORDER.indexOf(b.stance));
  return (
    <div>
      <PageHeader title="Preferences">What JobHunt believes you want: what you told it, then what it learned.</PageHeader>

      <StatementForm action={tellPreferences} />

      <Section title="You told JobHunt" id="stated" description="These always win over anything learned.">
        {taste.stated.length === 0 ? (
          <p className="text-muted">Nothing yet. Say what you want above.</p>
        ) : (
          <div className="grid gap-6 sm:grid-cols-2">
            {GROUPS.map((group) => {
              const prefs = byCategory(group.category);
              if (prefs.length === 0) return null;
              return (
                <div key={group.category}>
                  <h3 className="text-xs font-semibold uppercase tracking-wider text-muted">{group.title}</h3>
                  <ul className="mt-2 space-y-2">
                    {prefs.map((p) => (
                      <li key={p.id} className="text-[0.95rem]">
                        <p>
                          <span className="font-medium">{STANCE_LABEL[p.stance] ?? p.stance}:</span> {p.value}
                        </p>
                        <p className="text-xs text-muted">
                          {p.snippet ? (
                            <>
                              from your words <q>{p.snippet}</q>
                            </>
                          ) : (
                            "set by you"
                          )}
                          {p.certainty === "uncertain" && <span className="text-caution"> · JobHunt isn&apos;t sure it read this right{p.note ? `: ${p.note}` : ""}</span>}
                          {" · "}
                          <RemovePreference id={p.id} label={`${p.stance} ${p.value}`} remove={removePreference} />
                        </p>
                      </li>
                    ))}
                  </ul>
                </div>
              );
            })}
          </div>
        )}
      </Section>

      <Section title="Set something precisely" id="precise">
        <PreciseForm set={setPreference} />
      </Section>

      <Section title="Learned from your feedback" id="learned">
        <LearnedTaste taste={taste} />
      </Section>

      {taste.statements.length > 0 && (
        <Section title="In your words" id="statements" description="Everything you said, verbatim.">
          <ul className="space-y-3">
            {[...taste.statements].reverse().map((s) => (
              <li key={s.id} className="text-sm">
                <q>{s.text}</q>
                {s.not_understood.length > 0 && (
                  <p className="mt-0.5 text-muted">
                    Not interpreted: {s.not_understood.map((part) => `“${part}”`).join(", ")}
                  </p>
                )}
              </li>
            ))}
          </ul>
        </Section>
      )}
    </div>
  );
}
