import type { Metadata } from "next";
import type { ReactNode } from "react";

import { clarifyPreference, describeTaste, reviewTaste, updatePreferences, uploadResume } from "@/app/actions";
import { ClarifyPreference } from "@/components/clarify";
import { LocationControls, PreferenceEditing, WorkControls } from "@/components/preference-controls";
import { ResumeUpload } from "@/components/resume-upload";
import { RolesForm } from "@/components/roles";
import { Disclosure, RowGroup } from "@/components/summary";
import { Constraints, DescribeForm, TasteSummary } from "@/components/taste-profile";
import { LinkButton, PageHeader, textLinkClass } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { clarifyTitle } from "@/lib/clarify";
import { QUESTION, rolesText } from "@/lib/roles";

export const metadata: Metadata = { title: "Welcome" };

type StepState = "done" | "active" | "todo";

const MARKER: Record<StepState, string> = {
  done: "bg-fg-secondary",
  active: "bg-fg",
  todo: "border border-fg-muted",
};

/**
 * One step: a marker, its title, then either a one-line summary (done,
 * with Change) or what to do now. Summaries start on the title's edge.
 */
function Step({ n, title, state, summary, change, children }: { n: number; title: string; state: StepState; summary?: ReactNode; change?: ReactNode; children?: ReactNode }) {
  return (
    <li className="border-t border-line-subtle py-4">
      <div className="flex min-h-7 items-center gap-3">
        <span aria-hidden="true" className={`size-[7px] shrink-0 rounded-[1px] ${MARKER[state]}`} />
        <h2 id={`step-${n}`} className={state === "active" ? "text-title-m" : `text-[14px] font-medium ${state === "todo" ? "text-fg-muted" : "text-fg"}`}>
          {title}
          {state === "done" && <span className="sr-only"> (done)</span>}
        </h2>
      </div>
      {summary && <div className="mt-0.5 pl-[19px] text-[13px] text-fg-muted">{summary}</div>}
      {change && <div className="mt-1 pl-[19px]">{change}</div>}
      {children && <div className="mt-3 pl-[19px] max-sm:pl-0">{children}</div>}
    </li>
  );
}

/**
 * Onboarding, to value quickly, in five short steps: a resume; the one
 * structured question, what kind of role (a few chips, answered in
 * seconds); anything else they care about, in their words (optional);
 * the practical constraints that rule jobs out; then Today. Nothing else
 * to configure; the rest can be refined later in Profile and Preferences.
 */
export default async function Welcome({ searchParams }: { searchParams: Promise<{ words?: string }> }) {
  const { words } = await searchParams;
  const profile = await loadOrNoProfile(() => api.profile());
  const hasProfile = profile !== "no_profile";
  const [taste, settings] = hasProfile ? await Promise.all([api.tasteProfile(), api.taste()]) : [undefined, undefined];
  const answered = taste?.roles.answered ?? false;
  const described = taste?.looking_for_source === "description";
  const skipped = words === "skip" && !described;
  // A practical constraint read from the person's words with an open
  // question (a pay without currency, remote: must or nice?) isn't in
  // effect the way they meant it until answered.
  const questions = hasProfile ? profile.preferences.filter((p) => p.active && p.clarify && (p.category === "location" || p.category === "compensation")) : [];
  const resume = hasProfile ? (profile.documents ?? []).find((d) => d.current) : undefined;
  const experiences = hasProfile ? profile.experiences.length : 0;
  const wordsDone = described || skipped;
  const ready = hasProfile && answered && wordsDone;
  const state = (done: boolean, reachable: boolean): StepState => (done ? "done" : reachable ? "active" : "todo");
  return (
    <div className="max-w-[560px]">
      <PageHeader title="Set up Narrow" />
      <ol className="border-b border-line-subtle">
        <Step
          n={1}
          title="Your career"
          state={hasProfile ? "done" : "active"}
          summary={
            hasProfile && (
              <>
                {resume?.file_name ?? "Resume"} · {experiences} {experiences === 1 ? "role" : "roles"} found
              </>
            )
          }
          change={
            hasProfile && (
              <Disclosure label="Change">
                <ResumeUpload action={uploadResume} hasResume />
              </Disclosure>
            )
          }
        >
          {!hasProfile && <ResumeUpload action={uploadResume} hasResume={false} />}
        </Step>
        <Step
          n={2}
          title={QUESTION}
          state={state(answered, hasProfile)}
          summary={taste && answered && [rolesText(taste.roles), taste.roles.title && `“${taste.roles.title}”`].filter(Boolean).join(" · ")}
          change={
            taste &&
            answered && (
              <Disclosure label="Change">
                <RolesForm roles={taste.roles} review={reviewTaste} submitLabel="Save" />
              </Disclosure>
            )
          }
        >
          {taste && !answered && <RolesForm roles={taste.roles} review={reviewTaste} />}
        </Step>
        <Step
          n={3}
          title="Anything else you care about?"
          state={state(wordsDone, hasProfile && answered)}
          summary={
            taste &&
            (described ? (
              <>
                <q>{taste.looking_for}</q>{" "}
                <a href="/preferences#looking-for" className={textLinkClass}>
                  Change
                </a>
              </>
            ) : (
              skipped && "Skipped: you can add it any time in Preferences."
            ))
          }
        >
          {taste && answered && !wordsDone && (
            <DescribeForm
              describe={describeTaste}
              submitLabel="Continue"
              skip={
                <a href="/welcome?words=skip" className={`${textLinkClass} text-[14px] max-sm:inline-flex max-sm:min-h-11 max-sm:items-center`}>
                  Skip for now
                </a>
              }
            />
          )}
          {taste && described && <TasteSummary profile={taste} review={reviewTaste} />}
        </Step>
        <Step n={4} title="Practical constraints" state={ready ? "active" : "todo"}>
          {taste && settings && ready && (
            <div className="flex flex-col gap-6">
              <PreferenceEditing>
                <Constraints items={taste.constraints} noted={taste.interpretation?.constraints_noted ?? []} heading={false}>
                  <div className="flex flex-col gap-6">
                    <RowGroup id="work" title="Work">
                      <WorkControls controls={settings.controls} update={updatePreferences} />
                    </RowGroup>
                    <RowGroup id="location" title="Location">
                      <LocationControls controls={settings.controls} update={updatePreferences} />
                    </RowGroup>
                  </div>
                </Constraints>
              </PreferenceEditing>
              {questions.length > 0 && (
                <section aria-labelledby="questions">
                  <h3 id="questions" className="text-[14px] font-semibold text-fg">
                    {questions.length === 1 ? "One thing to settle" : `${questions.length} things to settle`}
                  </h3>
                  <ul className="mt-2 space-y-4">
                    {questions.map((p) => (
                      <li key={p.id}>
                        <p className="text-[14px] text-fg-body">
                          {clarifyTitle(p.clarify!)}
                          {p.snippet && (
                            <span className="text-fg-muted">
                              {" "}
                              — from <q>{p.snippet}</q>
                            </span>
                          )}
                        </p>
                        <ClarifyPreference p={p} clarify={clarifyPreference} />
                      </li>
                    ))}
                  </ul>
                </section>
              )}
            </div>
          )}
        </Step>
        <Step n={5} title="See what's worth your time" state={hasProfile ? "active" : "todo"}>
          {hasProfile && (
            <LinkButton href="/today" variant={ready && questions.length === 0 ? "primary" : "secondary"} className="max-sm:h-11">
              Go to Today
            </LinkButton>
          )}
        </Step>
      </ol>
    </div>
  );
}
