import type { Metadata } from "next";
import type { ReactNode } from "react";

import { clarifyPreference, describeTaste, reviewTaste, uploadResume } from "@/app/actions";
import { ClarifyPreference } from "@/components/clarify";
import { Constraints, DescribeForm, TasteSummary } from "@/components/taste-profile";
import { ResumeUpload } from "@/components/resume-upload";
import { Disclosure } from "@/components/summary";
import { LinkButton, PageHeader, textLinkClass } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { clarifyTitle } from "@/lib/clarify";

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
 * Onboarding, to value quickly: a resume, one question about what you
 * want (Narrow reads it into a short summary to confirm or correct), then
 * Today. Nothing to configure; the rest can be refined later in Profile
 * and Preferences.
 */
export default async function Welcome() {
  const profile = await loadOrNoProfile(() => api.profile());
  const hasProfile = profile !== "no_profile";
  const taste = hasProfile ? await api.tasteProfile() : undefined;
  const described = taste?.looking_for_source === "description";
  // A practical constraint read from the person's words with an open
  // question (a pay without currency, remote: must or nice?) isn't in
  // effect the way they meant it until answered.
  const questions = hasProfile ? profile.preferences.filter((p) => p.active && p.clarify && (p.category === "location" || p.category === "compensation")) : [];
  const resume = hasProfile ? (profile.documents ?? []).find((d) => d.current) : undefined;
  const experiences = hasProfile ? profile.experiences.length : 0;
  const wantsDone = described && questions.length === 0;
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
        <Step n={2} title="What you want" state={!hasProfile ? "todo" : wantsDone ? "done" : "active"}>
          {hasProfile && taste && (
            <>
              {described ? (
                <div className="flex flex-col gap-6">
                  <p className="text-[14px] text-fg-body">
                    <q>{taste.looking_for}</q>{" "}
                    <a href="/preferences#looking-for" className={textLinkClass}>
                      Change
                    </a>
                  </p>
                  <TasteSummary profile={taste} review={reviewTaste} />
                  <Constraints items={taste.constraints} noted={taste.interpretation?.constraints_noted ?? []} />
                </div>
              ) : (
                <DescribeForm describe={describeTaste} submitLabel="Continue" />
              )}
              {questions.length > 0 && (
                <section aria-labelledby="questions" className="mt-6">
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
            </>
          )}
        </Step>
        <Step n={3} title="See what's worth your time" state={hasProfile ? "active" : "todo"}>
          {hasProfile && (
            <LinkButton href="/today" variant={wantsDone ? "primary" : "secondary"} className="max-sm:h-11">
              Go to Today
            </LinkButton>
          )}
        </Step>
      </ol>
    </div>
  );
}
