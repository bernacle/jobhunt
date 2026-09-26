import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { removePreference, setPreference, tellPreferences } from "@/app/actions";
import { AddPreference, StatementForm } from "@/components/preferences";
import { NotInUse, TasteTable } from "@/components/taste";
import { PageHeader, Section } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";

export const metadata: Metadata = { title: "Preferences" };

export default async function PreferencesPage() {
  const taste = await loadOrNoProfile(() => api.taste());
  if (taste === "no_profile") redirect("/welcome");
  return (
    <div>
      <PageHeader title="Preferences">
        What you tell Narrow always wins. What it has learned from your decisions only changes the order.
      </PageHeader>

      <Section title="Add a preference" id="add" description="Pick what it's about and the rule, then the value.">
        <AddPreference set={setPreference} />
      </Section>

      <section aria-labelledby="taste-heading" className="mt-14 max-sm:mt-10">
        <h2 id="taste-heading" className="sr-only">
          You told us, and what we&apos;ve learned
        </h2>
        <TasteTable taste={taste} remove={removePreference} />
        <NotInUse taste={taste} />
        <p className="mt-4 text-[13px] leading-normal text-fg-muted">
          Learned from {taste.feedback_events} {taste.feedback_events === 1 ? "decision" : "decisions"} on {taste.opportunities}{" "}
          {taste.opportunities === 1 ? "job" : "jobs"}. Not for me decisions shape what Narrow learns. Not now doesn&apos;t.
        </p>
      </section>

      <Section
        title="In your words"
        id="statements"
        description="Say it however you like. Narrow keeps it word for word and shows what it read from it."
        className="mt-14"
      >
        <StatementForm action={tellPreferences} label="Describe what you're looking for" />
        {taste.statements.length > 0 && (
          <ul className="mt-6 border-t border-line-subtle">
            {[...taste.statements].reverse().map((s) => (
              <li key={s.id} className="border-b border-line-subtle py-3 text-[14px]">
                <q className="text-fg-body">{s.text}</q>
                {s.not_understood.length > 0 && (
                  <p className="mt-1 text-[13px] text-fg-muted">
                    Not interpreted: {s.not_understood.map((part) => `“${part}”`).join(", ")}
                  </p>
                )}
              </li>
            ))}
          </ul>
        )}
      </Section>
    </div>
  );
}
