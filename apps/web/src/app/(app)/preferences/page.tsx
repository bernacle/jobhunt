import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { clarifyPreference, removePreference, setPreference, tellPreferences, updatePreferences } from "@/app/actions";
import { ClarifyPreference } from "@/components/clarify";
import { CompanyControls, LayerLegend, LocationControls, PayControls, WorkControls } from "@/components/preference-controls";
import { AddPreference, StatementForm } from "@/components/preferences";
import { NotInUse, TasteTable } from "@/components/taste";
import { PageHeader, Section } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { clarifyTitle } from "@/lib/clarify";

export const metadata: Metadata = { title: "Preferences" };

/**
 * Configuring Narrow's judgment: the person's words, the structured
 * settings those words fill in (one set of preferences, editable either
 * way), and what Narrow learned, each in its own layer.
 */
export default async function PreferencesPage() {
  const taste = await loadOrNoProfile(() => api.taste());
  if (taste === "no_profile") redirect("/welcome");
  const controls = taste.controls;
  const questions = taste.stated.filter((p) => p.clarify);
  return (
    <div>
      <PageHeader title="Preferences">
        What you tell Narrow always wins. What it has learned from your decisions only changes the order.
      </PageHeader>
      <LayerLegend />

      <Section
        title="In your words"
        id="statements"
        description="Say it however you like. Narrow fills in the settings below from it, and asks instead of guessing when something is unclear. Your words are kept as written."
        className="mt-10"
      >
        <StatementForm action={tellPreferences} label="Describe what you're looking for" />
        {questions.length > 0 && (
          <div className="mt-6">
            <h3 className="text-title-m">Needs your answer</h3>
            <p className="mt-1 text-[13px] text-fg-muted">Until you answer, these are used as the milder reading, never as a requirement.</p>
            <ul className="mt-2">
              {questions.map((p) => (
                <li key={p.id} className="border-b border-line-subtle py-3">
                  <p className="text-[14px] font-medium text-fg">{clarifyTitle(p.clarify!)}</p>
                  {p.snippet && (
                    <p className="text-caption text-fg-muted">
                      From your words: <q>{p.snippet}</q>
                    </p>
                  )}
                  <ClarifyPreference p={p} clarify={clarifyPreference} />
                </li>
              ))}
            </ul>
          </div>
        )}
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

      <Section title="Work" id="work" description="How you want to work, and whether you'd move. Two separate answers." className="mt-14 max-sm:mt-10">
        <WorkControls controls={controls} update={updatePreferences} />
      </Section>

      <Section
        title="Location"
        id="location"
        description="Where you live, where you may legally work, and where remote roles should reach. Kept apart from a job's own location."
        className="mt-14 max-sm:mt-10"
      >
        <LocationControls controls={controls} update={updatePreferences} />
      </Section>

      <Section title="Pay" id="pay" description="A minimum is a requirement; a target is a preference. Currencies are never assumed or converted." className="mt-14 max-sm:mt-10">
        <PayControls controls={controls} update={updatePreferences} />
      </Section>

      <Section title="Company & team" id="company" description="The team you'd join, the company's size and its stage are three different things." className="mt-14 max-sm:mt-10">
        <CompanyControls controls={controls} update={updatePreferences} />
      </Section>

      <Section title="Roles, domains and more" id="add" description="Anything else, as one precise preference." className="mt-14 max-sm:mt-10">
        <AddPreference set={setPreference} />
      </Section>

      <Section
        title="Everything Narrow uses"
        id="taste"
        description="What you told Narrow beside what it learned from your decisions, so a tendency can never pass for a requirement."
        className="mt-14 max-sm:mt-10"
      >
        <TasteTable taste={taste} remove={removePreference} />
        <NotInUse taste={taste} />
        <p className="mt-4 text-[13px] leading-normal text-fg-muted">
          Learned from {taste.feedback_events} {taste.feedback_events === 1 ? "decision" : "decisions"} on {taste.opportunities}{" "}
          {taste.opportunities === 1 ? "job" : "jobs"}. Not for me decisions shape what Narrow learns. Not now doesn&apos;t.
        </p>
      </Section>
    </div>
  );
}
