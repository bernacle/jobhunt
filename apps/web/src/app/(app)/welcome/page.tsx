import type { Metadata } from "next";
import type { ReactNode } from "react";

import { tellPreferences, uploadResume } from "@/app/actions";
import { StatementForm } from "@/components/preferences";
import { ResumeUpload } from "@/components/resume-upload";
import { LinkButton, PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";

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
  return (
    <div className="max-w-[640px]">
      <PageHeader title="Let's find the few jobs worth your time">Two quick steps. Narrow does the searching from there.</PageHeader>
      <ol>
        <Step n={1} title="Your resume" done={hasProfile} description="So Narrow knows what you've done and which jobs you can take.">
          <ResumeUpload action={uploadResume} hasResume={hasProfile} />
        </Step>
        <Step n={2} title="What you want" done={hasPreferences} description="A sentence is enough. You can refine it any time in Preferences.">
          <StatementForm action={tellPreferences} />
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
