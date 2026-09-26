import type { Metadata } from "next";
import Link from "next/link";
import { redirect } from "next/navigation";

import { decideClaim, uploadResume } from "@/app/actions";
import { ClaimReview } from "@/components/claim-review";
import { ResumeUpload } from "@/components/resume-upload";
import { Consideration, Inferred } from "@/components/trust";
import { FactRows, PageHeader, Section, textLinkClass } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import type { Signal } from "@/lib/api-types";
import { ago } from "@/lib/format";

export const metadata: Metadata = { title: "Profile" };

const STRENGTH: Record<string, string> = {
  demonstrated: "used in your work",
  user_stated: "you said so",
  listed: "only in a skills list",
};

function signals(list: Signal[]) {
  return list.length === 0 ? (
    <span className="text-missing">None found yet</span>
  ) : (
    <Inferred>{list.map((s) => `${s.name} (${s.evidence})`).join(", ")}</Inferred>
  );
}

function Stale() {
  return <span className="ml-2 font-mono text-mono-xs font-normal text-warning">no longer in your resume</span>;
}

/**
 * What Narrow knows about the person, and where each fact came from.
 * Evidence over invention: unsure claims wait for review, and are never
 * used until confirmed.
 */
export default async function ProfilePage() {
  const data = await loadOrNoProfile(async () => {
    const [profile, claims] = await Promise.all([api.profile(), api.claims()]);
    return { profile, claims };
  });
  if (data === "no_profile") redirect("/welcome");
  const { profile, claims } = data;
  const now = new Date();
  const documents = profile.documents ?? [];
  const projects = profile.projects ?? [];
  const education = profile.education ?? [];
  const current = documents.find((d) => d.current);
  const counts = [
    `${profile.experiences.length} ${profile.experiences.length === 1 ? "experience" : "experiences"}`,
    `${profile.technologies.length} ${profile.technologies.length === 1 ? "skill" : "skills"}`,
    `${claims.total} to review`,
  ];
  const row = "grid gap-x-4 gap-y-1 border-t border-line-subtle py-3.5 sm:grid-cols-[minmax(0,1fr)_auto]";
  return (
    <div className="max-w-[640px]">
      <PageHeader title="Profile">
        What Narrow knows about you and where each fact came from. Contact details are never part of it, and a claim Narrow isn&apos;t
        sure about is never used until you confirm it.
      </PageHeader>
      <p className="-mt-4 mb-11 font-mono text-mono-s text-fg-muted max-sm:-mt-2 max-sm:mb-8">{counts.join(" · ")}</p>

      {claims.total > 0 && (
        <Section title="Needs your review" id="review" description="Narrow read these but isn't sure they're right. Nothing here is used until you confirm it.">
          <ClaimReview claims={claims.claims} total={claims.total} decide={decideClaim} />
        </Section>
      )}

      <Section title="Where you are" id="location">
        <FactRows
          rows={[
            { k: "Based in", v: profile.location ?? <span className="text-missing">Not stated. Eligibility can&apos;t be checked without it.</span> },
            ...(profile.headline ? [{ k: "Headline", v: profile.headline }] : []),
          ]}
        />
      </Section>

      <Section title="Sources" id="sources">
        {documents.length > 0 && (
          <ul role="list" className="mb-5">
            {documents.map((d) => (
              <li key={d.id} className="flex min-h-14 flex-col justify-center border-t border-line-subtle py-2.5">
                <p className="text-[14px] font-medium">
                  {d.file_name ?? "resume"}
                  {d.current && <span className="ml-2 font-mono text-mono-xs font-normal text-fg-muted">current</span>}
                </p>
                <p className="mt-0.5 font-mono text-mono-s text-fg-muted">
                  Imported {ago(d.last_imported_at, now)}
                  {d.pages ? ` · ${d.pages} ${d.pages === 1 ? "page" : "pages"}` : ""} · {d.kind}
                </p>
              </li>
            ))}
          </ul>
        )}
        <ResumeUpload action={uploadResume} hasResume={Boolean(current)} />
      </Section>

      <Section title="Experience" id="experience">
        {profile.experiences.length === 0 ? (
          <p className="text-[14px] text-fg-muted">No experience found yet.</p>
        ) : (
          <ol>
            {profile.experiences.map((e) => (
              <li key={e.id} className={row}>
                <div className="min-w-0">
                  <p className="text-[14.5px] leading-[1.4] font-semibold">
                    {e.title ?? "Role not stated"}
                    {e.company && <span className="font-normal text-fg-secondary"> · {e.company}</span>}
                    {e.stale && <Stale />}
                  </p>
                  {e.technologies.length > 0 && <p className="mt-1 text-row text-fg-secondary">{e.technologies.join(", ")}</p>}
                  {e.domains.length > 0 && <p className="text-row text-fg-muted">Domains: {e.domains.join(", ")}</p>}
                </div>
                <p className="font-mono text-mono-s whitespace-nowrap text-fg-muted sm:text-right">{e.period ?? "dates not stated"}</p>
              </li>
            ))}
          </ol>
        )}
      </Section>

      {(projects.length > 0 || education.length > 0) && (
        <Section title="Projects and education" id="projects">
          <ul role="list">
            {projects.map((p) => (
              <li key={p.id} className={row}>
                <div className="min-w-0">
                  <p className="text-[14px] leading-[1.4] font-medium">
                    {p.name}
                    {p.stale && <Stale />}
                  </p>
                  {p.description && <p className="mt-0.5 text-row text-fg-secondary">{p.description}</p>}
                  {p.technologies.length > 0 && <p className="text-row text-fg-muted">{p.technologies.join(", ")}</p>}
                </div>
                {p.period && <p className="font-mono text-mono-s whitespace-nowrap text-fg-muted sm:text-right">{p.period}</p>}
              </li>
            ))}
            {education.map((e) => (
              <li key={e.id} className={row}>
                <div className="min-w-0">
                  <p className="text-[14px] leading-[1.4] font-medium">
                    {e.institution}
                    {e.stale && <Stale />}
                  </p>
                  {(e.degree || e.field) && <p className="mt-0.5 text-row text-fg-secondary">{[e.degree, e.field].filter(Boolean).join(", ")}</p>}
                </div>
                {e.period && <p className="font-mono text-mono-s whitespace-nowrap text-fg-muted sm:text-right">{e.period}</p>}
              </li>
            ))}
          </ul>
        </Section>
      )}

      <Section title="Skills and technologies" id="skills" description="Strongest evidence first.">
        {profile.technologies.length === 0 ? (
          <p className="text-[14px] text-fg-muted">None found yet.</p>
        ) : (
          <ul role="list" className="border-t border-line-subtle">
            {profile.technologies.map((t) => (
              <li key={t.name} className="flex flex-wrap items-baseline justify-between gap-x-4 border-b border-line-subtle py-2 text-row">
                <span className={t.strength === "listed" ? "text-fg-secondary" : "font-medium text-fg"}>{t.name}</span>
                <span className="font-mono text-mono-s text-fg-muted">
                  {STRENGTH[t.strength] ?? t.strength}
                  {t.last_used && ` · ${t.last_used === "current" ? "current" : `last ${t.last_used}`}`}
                </span>
              </li>
            ))}
          </ul>
        )}
      </Section>

      <Section title="Career signals" id="signals" description="Narrow's reading of your history, with how much evidence backs each.">
        <FactRows
          rows={[
            { k: "Domains", v: signals(profile.domains) },
            { k: "Kinds of role", v: signals(profile.role_signals) },
            { k: "Seniority and ownership", v: signals(profile.ownership_signals) },
          ]}
        />
      </Section>

      {profile.gaps.length > 0 && (
        <Section title="Missing or uncertain" id="gaps">
          <ul role="list" className="space-y-1.5">
            {profile.gaps.map((g) => (
              <Consideration key={g} kind="missing">
                {g}
              </Consideration>
            ))}
          </ul>
        </Section>
      )}

      <Section title="Your data" id="data">
        <p className="text-[14px] text-fg-secondary">
          <a href="/api/export" className={textLinkClass}>
            Export your Narrow data
          </a>{" "}
          as a portable file: profile, preferences and feedback. Account, notifications and AI assistants are in{" "}
          <Link href="/settings" className={textLinkClass}>
            Settings
          </Link>
          .
        </p>
      </Section>
    </div>
  );
}
