import type { Metadata } from "next";
import { redirect } from "next/navigation";

import { importGithub, removeSource, uploadLinkedin, uploadResume } from "@/app/actions";
import { ResumeUpload } from "@/components/resume-upload";
import { GithubImport, LinkedinUpload, RemoveSource } from "@/components/source-import";
import { Disclosure, RowGroup, SummaryRow } from "@/components/summary";
import { Consideration, Inferred } from "@/components/trust";
import { LinkButton, PageHeader } from "@/components/ui";
import { api, loadOrNoProfile } from "@/lib/api";
import type { DocumentSummary, Signal } from "@/lib/api-types";
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
  return <span className="ml-2 font-mono text-mono-xs font-normal text-warning">no longer in its source</span>;
}

const SOURCE: Record<string, string> = { resume: "resume", linkedin: "LinkedIn", github: "GitHub" };

/** Where a record comes from, when it is more than the resume alone. */
function Sources({ sources }: { sources?: string[] }) {
  const list = sources ?? [];
  if (list.length === 0 || (list.length === 1 && list[0] === "resume")) return null;
  return <span className="ml-2 font-mono text-mono-xs font-normal text-fg-muted">{list.map((s) => SOURCE[s] ?? s).join(" + ")}</span>;
}

function documentTitle(d: DocumentSummary): string {
  if (d.source === "linkedin") return "LinkedIn export";
  if (d.source === "github") return "GitHub";
  return d.file_name ?? "resume";
}

function documentDetail(d: DocumentSummary): string {
  if (d.source === "linkedin" || d.source === "github") return d.file_name ?? "";
  return [d.pages ? `${d.pages} ${d.pages === 1 ? "page" : "pages"}` : "", d.kind].filter(Boolean).join(" · ");
}

/**
 * What Narrow knows about the person: experience, skills, projects, where
 * it came from. Claims it isn't sure of wait for review on their own page,
 * one row away, and are never used until confirmed.
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
  const current = documents.find((d) => d.current && (d.source ?? "resume") === "resume");
  const linkedin = documents.find((d) => d.source === "linkedin");
  const github = documents.find((d) => d.source === "github");
  const used = profile.technologies.filter((t) => t.strength !== "listed");
  const listed = profile.technologies.filter((t) => t.strength === "listed");
  const row = "grid gap-x-4 gap-y-1 border-t border-line-subtle py-3 sm:grid-cols-[minmax(0,1fr)_auto]";
  return (
    <div className="max-w-[640px]">
      <PageHeader title="Profile" />

      <div className="flex flex-col gap-11 max-sm:gap-9">
        {claims.total > 0 && (
          <div className="flex items-center justify-between gap-4 rounded-lg border border-line py-3.5 pr-4 pl-[18px] max-sm:flex-col max-sm:items-stretch max-sm:p-4">
            <div className="min-w-0">
              <p className="text-[14.5px] leading-[1.4] font-medium text-fg">
                {claims.total} {claims.total === 1 ? "claim needs" : "claims need"} your review
              </p>
              <p className="mt-0.5 text-[12.5px] text-fg-muted">Not used until you confirm.</p>
            </div>
            <LinkButton href="/profile/review" className="max-sm:h-11">
              Review
            </LinkButton>
          </div>
        )}

        <RowGroup id="about" title="About you">
          <SummaryRow
            id="based-in"
            label="Based in"
            value={profile.location ?? "Not stated"}
            unset={!profile.location}
            importance={!profile.location ? "Eligibility can't be checked without it" : undefined}
          />
          {profile.headline && <SummaryRow id="headline" label="Headline" value={profile.headline} />}
        </RowGroup>

        <RowGroup id="experience" title="Experience">
          {profile.experiences.length === 0 ? (
            <p className="border-t border-line-subtle py-3 text-[14px] text-fg-muted">No experience found yet.</p>
          ) : (
            <ol>
              {profile.experiences.map((e) => (
                <li key={e.id} className={row}>
                  <div className="min-w-0">
                    <p className="text-[14px] leading-[1.4] font-medium">
                      {e.company ?? "Company not stated"}
                      <span className="font-normal text-fg-secondary"> · {e.title ?? "role not stated"}</span>
                      {e.stale && <Stale />}
                      <Sources sources={e.sources} />
                    </p>
                    {e.technologies.length > 0 && <p className="mt-0.5 text-row text-fg-secondary">{e.technologies.join(", ")}</p>}
                  </div>
                  <p className="font-mono text-mono-s whitespace-nowrap text-fg-muted sm:text-right">{e.period ?? "dates not stated"}</p>
                </li>
              ))}
            </ol>
          )}
        </RowGroup>

        <RowGroup id="skills" title="Skills">
          <div className="border-t border-line-subtle py-3">
            {profile.technologies.length === 0 ? (
              <p className="text-[14px] text-fg-muted">None found yet.</p>
            ) : (
              <>
                {used.length > 0 && <p className="text-[14px] leading-[1.6] text-fg-body">{used.map((t) => t.name).join(", ")}</p>}
                {listed.length > 0 && <p className="mt-1 text-[13.5px] leading-[1.6] text-fg-secondary">Only in a skills list: {listed.map((t) => t.name).join(", ")}</p>}
                <Disclosure label="Evidence for each skill" className="mt-2">
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
                </Disclosure>
              </>
            )}
          </div>
        </RowGroup>

        {(projects.length > 0 || education.length > 0) && (
          <RowGroup id="projects" title="Projects and education">
            <ul role="list">
              {projects.map((p) => (
                <li key={p.id} className={row}>
                  <div className="min-w-0">
                    <p className="text-[14px] leading-[1.4] font-medium">
                      {p.name}
                      {p.stale && <Stale />}
                      <Sources sources={p.sources} />
                    </p>
                    {p.description && <p className="mt-0.5 text-row text-fg-secondary">{p.description}</p>}
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
          </RowGroup>
        )}

        <RowGroup id="signals" title="How Narrow reads your history">
          <SummaryRow id="signal-domains" label="Domains" value={signals(profile.domains)} />
          <SummaryRow id="signal-roles" label="Kinds of role" value={signals(profile.role_signals)} />
          <SummaryRow id="signal-seniority" label="Seniority and ownership" value={signals(profile.ownership_signals)} />
          {profile.gaps.length > 0 && (
            <div className="border-t border-line-subtle py-3">
              <Disclosure label="Missing or uncertain">
                <ul role="list" className="space-y-1.5">
                  {profile.gaps.map((g) => (
                    <Consideration key={g} kind="missing">
                      {g}
                    </Consideration>
                  ))}
                </ul>
              </Disclosure>
            </div>
          )}
        </RowGroup>

        <RowGroup id="sources" title="Sources">
          {documents.map((d) => {
            const removable = d.source === "linkedin" || d.source === "github" ? d.source : null;
            return (
              <div key={d.id} className="flex min-h-[var(--nr-row-min)] flex-col justify-center border-t border-line-subtle py-2.5">
                <div className="flex items-center justify-between gap-4">
                  <div className="min-w-0">
                    <p className="text-[14px] font-medium">
                      {documentTitle(d)}
                      {d.current && !removable && <span className="ml-2 font-mono text-mono-xs font-normal text-fg-muted">current</span>}
                    </p>
                    <p className="mt-0.5 font-mono text-mono-s text-fg-muted">
                      Imported {ago(d.last_imported_at, now)}
                      {documentDetail(d) && ` · ${documentDetail(d)}`}
                    </p>
                  </div>
                  {removable && <RemoveSource source={removable} label={documentTitle(d)} remove={removeSource} />}
                </div>
              </div>
            );
          })}
          <div className="border-t border-line-subtle py-3">
            {current ? (
              <Disclosure label="Replace resume">
                <ResumeUpload action={uploadResume} hasResume />
              </Disclosure>
            ) : (
              <ResumeUpload action={uploadResume} hasResume={false} />
            )}
          </div>
          <div className="border-t border-line-subtle py-3">
            <Disclosure label={linkedin ? "Update from LinkedIn" : "Add your LinkedIn export"}>
              <LinkedinUpload action={uploadLinkedin} imported={Boolean(linkedin)} />
            </Disclosure>
          </div>
          <div className="border-t border-line-subtle py-3">
            <Disclosure label={github ? "Update from GitHub" : "Add your GitHub"}>
              <GithubImport action={importGithub} login={github?.file_name?.replace(/^github\.com\//, "")} />
            </Disclosure>
          </div>
        </RowGroup>
      </div>
    </div>
  );
}
