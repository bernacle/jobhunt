import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { decideClaim, uploadResume } from "@/app/actions";
import { ClaimReview } from "@/components/claim-review";
import { ResumeUpload } from "@/components/resume-upload";
import { PageHeader, Section } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import { ago } from "@/lib/format";

export const metadata: Metadata = { title: "Profile" };

const STRENGTH: Record<string, string> = {
  demonstrated: "used in your work",
  user_stated: "you said so",
  listed: "only in a skills list",
};

export default async function ProfilePage() {
  const data = await loadOrNoProfile(async () => {
    const [profile, claims] = await Promise.all([api.profile(), api.claims()]);
    return { profile, claims };
  });
  if (data === "no_profile") redirect("/welcome");
  const { profile, claims } = data;
  const documents = profile.documents ?? [];
  const projects = profile.projects ?? [];
  const education = profile.education ?? [];
  const current = documents.find((d) => d.current);
  return (
    <div>
      <PageHeader title="Profile">
        The professional picture JobHunt ranks against. Contact details are never part of it.
      </PageHeader>

      <Section title="Resume" id="resume">
        {current && (
          <p className="mb-4 text-sm">
            Current: <span className="font-medium">{current.file_name ?? "resume"}</span>
            <span className="text-muted">
              {" "}
              · imported {ago(current.last_imported_at)}
              {documents.length > 1 && ` · ${documents.length} versions imported`}
            </span>
          </p>
        )}
        <ResumeUpload action={uploadResume} hasResume={Boolean(current)} />
        <p className="mt-3 text-sm">
          <a href="/api/export" className="underline">
            Export your JobHunt data
          </a>{" "}
          <span className="text-muted">(profile, preferences and feedback, as a portable file)</span>
        </p>
      </Section>

      {claims.total > 0 && (
        <Section
          title="Needs your review"
          id="review"
          description="Claims JobHunt isn't sure about. Confirm what's true; reject what isn't."
        >
          <ClaimReview claims={claims.claims} total={claims.total} decide={decideClaim} />
        </Section>
      )}

      <Section title="Experience" id="experience">
        {profile.experiences.length === 0 ? (
          <p className="text-muted">No experience found yet.</p>
        ) : (
          <ol className="space-y-4">
            {profile.experiences.map((e) => (
              <li key={e.id}>
                <p className="font-medium">
                  {[e.title, e.company].filter(Boolean).join(" · ")}
                  {e.stale && <span className="ml-2 text-xs font-normal text-caution">no longer in your resume</span>}
                </p>
                {e.period && <p className="text-sm text-muted">{e.period}</p>}
                {e.technologies.length > 0 && <p className="mt-1 text-sm">{e.technologies.join(", ")}</p>}
                {e.domains.length > 0 && <p className="text-sm text-muted">Domains: {e.domains.join(", ")}</p>}
              </li>
            ))}
          </ol>
        )}
      </Section>

      {projects.length > 0 && (
        <Section title="Projects" id="projects">
          <ul className="space-y-3">
            {projects.map((p) => (
              <li key={p.id}>
                <p className="font-medium">{p.name}</p>
                {p.description && <p className="text-sm text-muted">{p.description}</p>}
                {p.technologies.length > 0 && <p className="text-sm">{p.technologies.join(", ")}</p>}
              </li>
            ))}
          </ul>
        </Section>
      )}

      {education.length > 0 && (
        <Section title="Education" id="education">
          <ul className="space-y-2">
            {education.map((e) => (
              <li key={e.id}>
                <p className="font-medium">{e.institution}</p>
                <p className="text-sm text-muted">{[e.degree, e.field, e.period].filter(Boolean).join(" · ")}</p>
              </li>
            ))}
          </ul>
        </Section>
      )}

      <Section title="Skills and technologies" id="skills" description="Strongest evidence first.">
        {profile.technologies.length === 0 ? (
          <p className="text-muted">None found yet.</p>
        ) : (
          <ul className="grid gap-x-6 gap-y-1.5 sm:grid-cols-2">
            {profile.technologies.map((t) => (
              <li key={t.name} className="text-sm">
                <span className="font-medium">{t.name}</span>{" "}
                <span className="text-muted">
                  — {STRENGTH[t.strength] ?? t.strength}
                  {t.last_used && `, ${t.last_used === "current" ? "current" : `last ${t.last_used}`}`}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Signals" id="signals" description="What your experience shows, and how much evidence backs each.">
        <dl className="grid gap-4 text-sm sm:grid-cols-3">
          {[
            ["Domains", profile.domains],
            ["Kinds of role", profile.role_signals],
            ["Seniority and ownership", profile.ownership_signals],
          ].map(([label, signals]) => (
            <div key={label as string}>
              <dt className="text-xs font-semibold uppercase tracking-wider text-muted">{label as string}</dt>
              <dd className="mt-1">
                {(signals as { name: string; evidence: number }[]).length === 0
                  ? "—"
                  : (signals as { name: string; evidence: number }[])
                      .map((s) => `${s.name} (${s.evidence})`)
                      .join(", ")}
              </dd>
            </div>
          ))}
        </dl>
      </Section>

      {profile.gaps.length > 0 && (
        <Section title="Missing or uncertain" id="gaps">
          <ul className="list-disc space-y-1 pl-5 text-sm text-muted">
            {profile.gaps.map((g) => (
              <li key={g}>{g}</li>
            ))}
          </ul>
        </Section>
      )}
    </div>
  );
}
