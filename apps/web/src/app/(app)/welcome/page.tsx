import type { Metadata } from "next";

import { tellPreferences, uploadResume } from "@/app/actions";
import { StatementForm } from "@/components/preferences";
import { ResumeUpload } from "@/components/resume-upload";
import { LinkButton, PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";

export const metadata: Metadata = { title: "Welcome" };

/**
 * Onboarding, in three steps: a resume, a sentence about what you want,
 * then Today. The profile can be improved later.
 */
export default async function Welcome() {
  const profile = await loadOrNoProfile(() => api.profile());
  const hasProfile = profile !== "no_profile";
  const hasPreferences = hasProfile && profile.preferences.length > 0;
  return (
    <div>
      <PageHeader title="Let's find the few jobs worth your time">
        Two quick steps. JobHunt does the searching from there.
      </PageHeader>
      <ol className="space-y-10">
        <li aria-labelledby="step-1">
          <h2 id="step-1" className="font-serif text-xl">
            1. Your resume {hasProfile && <span className="text-base text-accent">— done</span>}
          </h2>
          <p className="mb-4 mt-1 text-sm text-muted">So JobHunt knows what you&apos;ve done and which jobs you can take.</p>
          <ResumeUpload action={uploadResume} hasResume={hasProfile} />
        </li>
        <li aria-labelledby="step-2">
          <h2 id="step-2" className="font-serif text-xl">
            2. What you want {hasPreferences && <span className="text-base text-accent">— done</span>}
          </h2>
          <p className="mb-4 mt-1 text-sm text-muted">A sentence is enough. You can refine it any time.</p>
          <StatementForm action={tellPreferences} />
        </li>
        <li aria-labelledby="step-3">
          <h2 id="step-3" className="font-serif text-xl">
            3. See what&apos;s worth your time
          </h2>
          <p className="mb-4 mt-1 text-sm text-muted">
            JobHunt checks job boards continuously and shows only the few openings that fit.
          </p>
          {hasProfile ? (
            <LinkButton href="/today" variant="primary">
              Go to Today
            </LinkButton>
          ) : (
            <p className="text-sm text-muted">Import a resume first.</p>
          )}
        </li>
      </ol>
    </div>
  );
}
