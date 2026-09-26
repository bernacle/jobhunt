import type { Metadata } from "next";
import type { ReactNode } from "react";

import { clarifyPreference, tellPreferences, uploadResume } from "@/app/actions";
import { ClarifyPreference } from "@/components/clarify";
import { StatementForm } from "@/components/preferences";
import { ResumeUpload } from "@/components/resume-upload";
import { LinkButton, PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { clarifyTitle } from "@/lib/clarify";

export const metadata: Metadata = { title: "Welcome" };

function Step({ n, title, done, description, children }: { n: number; title: string; done?: boolean; description: string; children: ReactNode }) {
  return (
    <li className="grid gap-x-6 border-t border-line-subtle py-7 sm:grid-cols-[40px_minmax(0,1fr)]">
      <span aria-hidden="true" className="pt-1 font-mono text-mono-s text-fg-muted">
        {String(n).padStart(2, "0")}
      </span>
      <div className="min-w-0">
        <h2 id={`step-${n}`} className="flex items-baseline gap-3 text-title-m">
          {title}
          {done && (
            <span className="text-[12px] font-medium text-accent">
              <span aria-hidden="true">✓ </span>Done
            </span>
          )}
        </h2>
        <p className="mt-1 mb-4 text-[13px] text-fg-muted">{description}</p>
        {children}
      </div>
    </li>
  );
}

/**
 * Onboarding, in three steps: a resume, a sentence about what you want,
 * then Today. The profile can be improved later.
 */
export default async function Welcome() {
  const profile = await loadOrNoProfile(() => api.profile());
  const hasProfile = profile !== "no_profile";
  const hasPreferences = hasProfile && profile.preferences.length > 0;
  // Read from the person's words with an open question: not in effect the
  // way they meant it until answered, so the step isn't done either.
  const questions = hasProfile ? profile.preferences.filter((p) => p.active && p.clarify) : [];
  return (
    <div className="max-w-[640px]">
      <PageHeader title="Let's find the few jobs worth your time">Two quick steps. Narrow does the searching from there.</PageHeader>
      <ol>
        <Step n={1} title="Your resume" done={hasProfile} description="So Narrow knows what you've done and which jobs you can take.">
          <ResumeUpload action={uploadResume} hasResume={hasProfile} />
        </Step>
        <Step
          n={2}
          title="What you want"
          done={hasPreferences && questions.length === 0}
          description="A sentence is enough. You can refine it any time in Preferences."
        >
          <StatementForm action={tellPreferences} />
          {questions.length > 0 && (
            <section aria-labelledby="questions" className="mt-6">
              <h3 id="questions" className="text-[14px] font-semibold text-fg">
                {questions.length === 1 ? "One thing to settle" : `${questions.length} things to settle`} before Narrow relies on them
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
        </Step>
        <Step n={3} title="See what's worth your time" description="Narrow keeps checking job boards and shows only the few openings that fit.">
          {hasProfile ? (
            <LinkButton href="/today" variant="primary" className="max-sm:h-11">
              Go to Today
            </LinkButton>
          ) : (
            <p className="text-[14px] text-fg-muted">Import a resume first.</p>
          )}
        </Step>
      </ol>
    </div>
  );
}
