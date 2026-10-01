import type { Metadata } from "next";
import { redirect } from "next/navigation";

import {
  clarifyPreference,
  describeTaste,
  removePreference,
  reviewTaste,
  setPreference,
  tellPreferences,
  updatePreferences,
} from "@/app/actions";
import { ClarifyPreference } from "@/components/clarify";
import { EvidenceTrigger } from "@/components/evidence-panel";
import {
  CompanyControls,
  LayerTerms,
  LocationControls,
  PayControls,
  PreferenceEditing,
  WorkControls,
} from "@/components/preference-controls";
import { AddPreferenceRow, InYourWords, RemovePreference } from "@/components/preferences";
import { TargetRoles } from "@/components/roles";
import { Disclosure, RowGroup, SummaryRow } from "@/components/summary";
import { TasteReview } from "@/components/taste";
import { Constraints, LearnedOverTime, LookingFor, TasteSummary } from "@/components/taste-profile";
import { PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import type { PreferenceView } from "@/lib/api-types";
import { clarifyTitle } from "@/lib/clarify";
import { STANCE_LABEL } from "@/lib/format";
import { controlRecordIds } from "@/lib/preferences";

export const metadata: Metadata = { title: "Preferences" };

const CATEGORY: Record<string, string> = {
  role: "Role",
  domain: "Domain",
  work_style: "Way of working",
  company: "Kind of company",
  timezone: "Time zone",
  location: "Location",
  compensation: "Pay",
};

/** Practical questions (pay, where you can work) come first; the rest can wait in Fine-tune. */
const PRACTICAL = new Set(["location", "compensation"]);

function Questions({ id, title, questions }: { id: string; title: string; questions: PreferenceView[] }) {
  if (questions.length === 0) return null;
  return (
    <section id={id} aria-labelledby={`${id}-heading`}>
      <h2 id={`${id}-heading`} className="text-[15px] leading-[1.4] font-semibold">
        {title}
      </h2>
      <ul className="mt-1.5">
        {questions.map((p) => (
          <li key={p.id} className="border-t border-line-subtle py-3">
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
    </section>
  );
}

/**
 * What Narrow understands about the person, not ranking settings: the
 * kinds of role they're looking for (asked until they answer), anything
 * else they care about in their words, a short summary of the kind of role
 * and company they want (and avoid) to confirm or correct, their practical
 * constraints, and what was learned. Every structured setting is still
 * there, under Fine-tune, for whoever wants it.
 */
export default async function PreferencesPage() {
  const loaded = await loadOrNoProfile(() => Promise.all([api.tasteProfile(), api.taste()]));
  if (loaded === "no_profile") redirect("/welcome");
  const [profile, taste] = loaded;
  const controls = taste.controls;
  const questions = taste.stated.filter((p) => p.clarify);
  const practicalQuestions = questions.filter((p) => PRACTICAL.has(p.category));
  const otherQuestions = questions.filter((p) => !PRACTICAL.has(p.category));
  const inControls = controlRecordIds(controls);
  const others = taste.stated.filter((p) => !inControls.has(p.id));
  const requirements = taste.stated.filter((p) => p.layer === "requirement").length;
  const preferences = taste.stated.length - requirements;
  const learned = taste.learned.length;
  return (
    <div className="max-w-[760px]">
      <PageHeader
        title="Preferences"
        aside={
          <EvidenceTrigger label="How preferences work" title="How preferences work">
            <LayerTerms />
          </EvidenceTrigger>
        }
      />
      <PreferenceEditing>
        <div className="flex flex-col gap-10 max-sm:gap-8">
          <TargetRoles profile={profile} review={reviewTaste} />

          <LookingFor profile={profile} describe={describeTaste} review={reviewTaste} />

          <TasteSummary profile={profile} review={reviewTaste} />

          <Questions id="questions" title="Needs your answer" questions={practicalQuestions} />

          <Constraints items={profile.constraints} noted={profile.interpretation?.constraints_noted ?? []}>
            <div className="flex flex-col gap-6">
              <RowGroup id="work" title="Work">
                <WorkControls controls={controls} update={updatePreferences} />
              </RowGroup>
              <RowGroup id="location" title="Location">
                <LocationControls controls={controls} update={updatePreferences} />
              </RowGroup>
              <RowGroup id="pay" title="Pay">
                <PayControls controls={controls} update={updatePreferences} />
              </RowGroup>
            </div>
          </Constraints>

          <LearnedOverTime items={profile.learned} />

          <section id="fine-tune" aria-label="Fine-tune">
            <Disclosure label="Fine-tune">
              <div className="flex flex-col gap-9">
                <p className="text-[13px] text-pretty text-fg-muted">
                  Every setting Narrow ranks with today. You rarely need these: describing what you want and correcting the summary above is enough.
                </p>

                <Questions id="other-questions" title="Also unclear" questions={otherQuestions} />

                <RowGroup id="company" title="Company & team">
                  <CompanyControls controls={controls} update={updatePreferences} />
                </RowGroup>

                <RowGroup id="add" title="Roles, domains and more">
                  {others.map((p) => (
                    <SummaryRow
                      key={p.id}
                      id={`pref-${p.id}`}
                      label={CATEGORY[p.category] ?? p.category}
                      value={
                        <>
                          {p.value}
                          {p.clarify && <span className="nr-inferred text-[12.5px] font-normal"> needs your answer</span>}
                        </>
                      }
                      importance={STANCE_LABEL[p.stance] ?? p.stance}
                      action={<RemovePreference id={p.id} label={`${p.stance} ${p.value}`} remove={removePreference} />}
                    />
                  ))}
                  <AddPreferenceRow set={setPreference} />
                </RowGroup>

                <InYourWords statements={taste.statements} action={tellPreferences} />

                <RowGroup id="taste" title="What Narrow uses">
                  <div className="flex min-h-[var(--nr-row-min)] items-center justify-between gap-4 border-t border-line-subtle py-2.5 max-sm:min-h-[var(--nr-row-min-touch)]">
                    <p className="flex flex-wrap gap-x-4 gap-y-0.5 text-[14px] text-fg-body nr-tnum max-sm:flex-col">
                      <span>
                        {requirements} {requirements === 1 ? "requirement" : "requirements"}
                      </span>
                      <span>
                        {preferences} {preferences === 1 ? "preference" : "preferences"}
                      </span>
                      <span className={learned === 0 ? "text-fg-muted" : ""}>{learned === 0 ? "Nothing learned yet" : `${learned} learned`}</span>
                    </p>
                    <EvidenceTrigger label="Review all" title="What Narrow uses" subtitle="What you said always wins over what Narrow learned.">
                      <TasteReview taste={taste} remove={removePreference} clarify={clarifyPreference} />
                    </EvidenceTrigger>
                  </div>
                </RowGroup>
              </div>
            </Disclosure>
          </section>
        </div>
      </PreferenceEditing>
    </div>
  );
}
