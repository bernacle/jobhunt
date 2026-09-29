//! Use-case tests for sources beyond the resume: a LinkedIn export and a
//! GitHub account feeding the same evidence graph, through
//! [`ProfileService`] and the in-memory repository. All people, companies
//! and repositories here are fictional.

use chrono::{DateTime, TimeZone, Utc};

use crate::memory::MemoryProfiles;
use crate::*;

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap()
}

fn d(s: &str) -> Option<PartialDate> {
    Some(s.parse().unwrap())
}

fn sha(seed: &str) -> String {
    let hex = jobhunt_core::StableId::derive("test.sha", &[seed]).to_hex();
    format!("{hex}{hex}")
}

fn resume_doc(name: &str) -> SourceDocument {
    SourceDocument {
        id: DocumentId::derive(&[name]),
        kind: DocumentKind::Text,
        file_name: Some(format!("{name}.txt")),
        sha256: sha(name),
        pages: Some(1),
        text: format!("text of {name}"),
        parser: "test".into(),
        first_imported_at: at(0),
        last_imported_at: at(0),
    }
}

fn linkedin_doc(seed: &str) -> SourceDocument {
    SourceDocument {
        id: DocumentId::derive(&["ignored"]),
        kind: DocumentKind::Linkedin,
        file_name: Some("Basic_LinkedInDataExport_09-01-2026.zip".into()),
        sha256: sha(seed),
        pages: None,
        text: format!("linkedin {seed}"),
        parser: "linkedin-export/1".into(),
        first_imported_at: at(0),
        last_imported_at: at(0),
    }
}

/// "Senior Software Engineer at Northwind Labs" as a resume writes it.
fn northwind_resume() -> ParsedExperience {
    ParsedExperience {
        company: Some("Northwind Labs".into()),
        title: Some("Senior Software Engineer".into()),
        start: d("2021-03"),
        current: true,
        header: "Northwind Labs — Senior Software Engineer  Mar 2021 – Present".into(),
        bullets: vec!["Built the settlement pipeline in Rust.".into()],
        tech_line: Some("Tech: Rust, PostgreSQL".into()),
        technologies: vec!["Rust".into(), "PostgreSQL".into()],
        ..ParsedExperience::default()
    }
}

/// The same position, as a LinkedIn export row.
fn northwind_linkedin(start: &str) -> ParsedExperience {
    ParsedExperience {
        company: Some("Northwind Labs".into()),
        title: Some("Senior Software Engineer".into()),
        start: d(start),
        current: true,
        location: Some("Lisbon, Portugal".into()),
        header: format!("Senior Software Engineer · Northwind Labs · {start} – Present"),
        bullets: vec!["Built the settlement pipeline in Rust.".into()],
        ..ParsedExperience::default()
    }
}

fn contoso(title: &str) -> ParsedExperience {
    ParsedExperience {
        company: Some("Contoso Freight".into()),
        title: Some(title.into()),
        start: d("2018-01"),
        end: d("2021-02"),
        header: format!("{title} · Contoso Freight · Jan 2018 – Feb 2021"),
        ..ParsedExperience::default()
    }
}

fn resume() -> ParsedResume {
    ParsedResume {
        basics: ParsedBasics {
            name: Some("Riley Example".into()),
            headline: Some("Backend engineer".into()),
            ..ParsedBasics::default()
        },
        experiences: vec![northwind_resume()],
        education: vec![ParsedEducation {
            institution: "Example State University".into(),
            degree: Some("B.Sc.".into()),
            field: Some("Computer Science".into()),
            start: d("2013"),
            end: d("2017"),
            header: "Example State University — B.Sc. in Computer Science  2013 – 2017".into(),
            ..ParsedEducation::default()
        }],
        skills: vec![ParsedSkillLine {
            category: Some("Languages".into()),
            skills: vec!["Rust".into(), "Go".into()],
            line: "Languages: Rust, Go".into(),
        }],
        ..ParsedResume::default()
    }
}

fn linkedin() -> ParsedResume {
    ParsedResume {
        basics: ParsedBasics {
            headline: Some("Staff-curious backend engineer".into()),
            summary: Some("I build payment systems.".into()),
            location: Some("Lisbon, Portugal".into()),
            ..ParsedBasics::default()
        },
        experiences: vec![northwind_linkedin("2021-03"), contoso("Software Engineer")],
        education: vec![ParsedEducation {
            institution: "Example State University".into(),
            degree: Some("B.Sc.".into()),
            start: d("2013"),
            end: d("2017"),
            header: "Example State University · B.Sc. · 2013 – 2017".into(),
            ..ParsedEducation::default()
        }],
        skills: vec![ParsedSkillLine {
            category: None,
            skills: vec!["Rust".into(), "Kafka".into()],
            line: "Skills: Rust, Kafka".into(),
        }],
        other: vec![ParsedOther {
            section: "Certifications".into(),
            line: "Certified Kafka Developer — Example Institute (Jan 2024)".into(),
        }],
        ..ParsedResume::default()
    }
}

fn claim<'a>(data: &'a ProfileData, text: &str) -> &'a Claim {
    data.claims
        .iter()
        .find(|c| c.text == text)
        .unwrap_or_else(|| panic!("no claim {text:?} in {:#?}", texts(data)))
}

fn texts(data: &ProfileData) -> Vec<&str> {
    data.claims.iter().map(|c| c.text.as_str()).collect()
}

fn employment<'a>(data: &'a ProfileData, company: &str) -> Vec<&'a Claim> {
    let ids: Vec<ExperienceId> = data
        .experiences
        .iter()
        .filter(|e| e.company.as_deref() == Some(company))
        .map(|e| e.id)
        .collect();
    data.claims
        .iter()
        .filter(|c| c.kind == ClaimKind::Employment)
        .filter(|c| matches!(c.subject, Subject::Experience(id) if ids.contains(&id)))
        .collect()
}

fn ids(data: &ProfileData) -> Vec<String> {
    let mut out: Vec<String> = data.claims.iter().map(|c| c.id.to_string()).collect();
    out.sort();
    out
}

async fn with_resume(repo: &MemoryProfiles) -> ProfileData {
    ProfileService::new(repo)
        .import_resume(resume_doc("resume"), &resume(), at(1))
        .await
        .unwrap()
        .1
}

async fn import_linkedin(
    repo: &MemoryProfiles,
    seed: &str,
    parsed: &ParsedResume,
    minute: u32,
) -> (ImportReport, ProfileData) {
    ProfileService::new(repo)
        .import_linkedin(linkedin_doc(seed), parsed, at(minute))
        .await
        .unwrap()
}

#[tokio::test]
async fn a_linkedin_export_alone_builds_a_reviewable_profile() {
    let repo = MemoryProfiles::default();
    let (report, data) = import_linkedin(&repo, "v1", &linkedin(), 1).await;
    assert!(report.first_import && !report.same_file);
    assert_eq!(report.experiences.added, 2);
    assert_eq!(data.documents.len(), 1);
    assert_eq!(data.documents[0].kind, DocumentKind::Linkedin);
    assert_eq!(
        data.documents[0].id,
        source_document_id(data.id(), Origin::Linkedin)
    );
    assert!(
        data.experiences
            .iter()
            .all(|e| e.meta.origin == Origin::Linkedin
                && e.meta.import_key.as_deref().unwrap().starts_with("li|exp|"))
    );
    // Basics the profile did not have are filled; the name is never read.
    assert_eq!(
        data.profile.headline.as_deref(),
        Some("Staff-curious backend engineer")
    );
    assert_eq!(data.profile.location.as_deref(), Some("Lisbon, Portugal"));
    assert_eq!(data.profile.name, None);

    // A structured position is a grounded fact, quoted from the export.
    let job = claim(
        &data,
        "Senior Software Engineer at Northwind Labs (Mar 2021 – Present)",
    );
    assert_eq!(job.provenance, Provenance::Extracted);
    assert!(data.standing(job).is_usable());
    assert_eq!(data.describe(job), "quoted from your LinkedIn export");
    assert_eq!(data.claim_sources(job), vec![Origin::Linkedin]);
    let source = job.source.as_ref().unwrap();
    assert_eq!(source.document, data.documents[0].id);
    assert_eq!(
        source.snippet,
        "Senior Software Engineer · Northwind Labs · 2021-03 – Present"
    );
    // Skills and certifications are claims; inferences still need review.
    assert!(data.skills.iter().any(|s| s.name == "Kafka"));
    let cert = claim(
        &data,
        "Certifications: Certified Kafka Developer — Example Institute (Jan 2024)",
    );
    assert!(data.standing(cert).is_usable());
    assert!(
        data.review_queue()
            .iter()
            .all(|c| c.provenance == Provenance::Inferred),
        "only inferences wait for review: {:?}",
        data.review_queue()
            .iter()
            .map(|c| &c.text)
            .collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn reimporting_the_same_export_changes_nothing() {
    let repo = MemoryProfiles::default();
    let (_, before) = import_linkedin(&repo, "v1", &linkedin(), 1).await;
    let (report, after) = import_linkedin(&repo, "v1", &linkedin(), 2).await;
    assert!(report.same_file && !report.first_import);
    assert_eq!(
        report.experiences,
        Tally {
            unchanged: 2,
            ..Tally::default()
        }
    );
    assert_eq!(report.claims.added, 0);
    assert_eq!(report.claims.updated, 0);
    assert_eq!(report.claims.stale, 0);
    assert_eq!(report.skills.added, 0);
    assert_eq!(ids(&before), ids(&after), "no duplicate or churned claims");
    assert_eq!(after.experiences.len(), 2);
    assert_eq!(after.skills.len(), before.skills.len());
    assert_eq!(after.documents.len(), 1, "one document, updated in place");
}

#[tokio::test]
async fn resume_and_linkedin_support_one_position() {
    let repo = MemoryProfiles::default();
    let before = with_resume(&repo).await;
    let resume_job = claim(
        &before,
        "Senior Software Engineer at Northwind Labs (Mar 2021 – Present)",
    )
    .id;
    let (report, data) = import_linkedin(&repo, "v1", &linkedin(), 2).await;

    // Northwind is one experience; Contoso (only on LinkedIn) is new.
    assert_eq!(report.experiences.corroborated, 1);
    assert_eq!(report.experiences.added, 1);
    let northwind: Vec<&Experience> = data
        .experiences
        .iter()
        .filter(|e| e.company.as_deref() == Some("Northwind Labs"))
        .collect();
    assert_eq!(northwind.len(), 1, "no duplicate position");
    assert_eq!(
        northwind[0].meta.origin,
        Origin::Resume,
        "the resume owns it"
    );
    assert_eq!(
        data.record_sources(&northwind[0].meta),
        vec![Origin::Resume, Origin::Linkedin]
    );
    assert_eq!(
        northwind[0].location, None,
        "LinkedIn does not rewrite fields"
    );

    // One employment claim, two sources; the resume's words stay.
    let jobs = employment(&data, "Northwind Labs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].id, resume_job);
    assert_eq!(
        data.claim_sources(jobs[0]),
        vec![Origin::Resume, Origin::Linkedin]
    );
    assert_eq!(jobs[0].corroborations.len(), 1);
    assert_eq!(
        jobs[0].corroborations[0].snippet,
        "Senior Software Engineer · Northwind Labs · 2021-03 – Present"
    );
    // The same bullet and the same skill are one claim each.
    assert_eq!(
        data.claims
            .iter()
            .filter(|c| c.text == "Built the settlement pipeline in Rust.")
            .count(),
        1
    );
    let rust_skill: Vec<&Claim> = data
        .claims
        .iter()
        .filter(|c| c.kind == ClaimKind::Skill && c.topic.as_deref() == Some("rust"))
        .collect();
    assert_eq!(rust_skill.len(), 1);
    assert_eq!(
        data.claim_sources(rust_skill[0]),
        vec![Origin::Resume, Origin::Linkedin]
    );
    // Education: one entry, two sources.
    assert_eq!(data.education.len(), 1);
    assert_eq!(
        data.record_sources(&data.education[0].meta),
        vec![Origin::Resume, Origin::Linkedin]
    );
    // The resume's basics win nothing new, and LinkedIn fills the gaps.
    assert_eq!(data.profile.headline.as_deref(), Some("Backend engineer"));
    assert_eq!(
        data.profile.summary.as_deref(),
        Some("I build payment systems.")
    );
    assert_eq!(report.conflicts, 0);

    // A resume re-import keeps LinkedIn's evidence (no churn, no stale).
    let (again, data) = ProfileService::new(&repo)
        .import_resume(resume_doc("resume"), &resume(), at(3))
        .await
        .unwrap();
    assert_eq!(again.claims.stale, 0);
    assert_eq!(again.skills.stale, 0);
    let jobs = employment(&data, "Northwind Labs");
    assert_eq!(
        data.claim_sources(jobs[0]),
        vec![Origin::Resume, Origin::Linkedin]
    );
    assert!(
        data.experiences.iter().all(|e| !e.meta.is_stale()),
        "a resume re-import does not stale LinkedIn's records"
    );
    assert!(
        data.skills
            .iter()
            .any(|s| s.name == "Kafka" && !s.meta.is_stale())
    );
}

#[tokio::test]
async fn linkedin_first_then_resume_also_converges() {
    let repo = MemoryProfiles::default();
    import_linkedin(&repo, "v1", &linkedin(), 1).await;
    let (report, data) = ProfileService::new(&repo)
        .import_resume(resume_doc("resume"), &resume(), at(2))
        .await
        .unwrap();
    assert_eq!(report.experiences.corroborated, 1);
    assert_eq!(report.experiences.added, 0);
    assert_eq!(
        data.experiences
            .iter()
            .filter(|e| e.company.as_deref() == Some("Northwind Labs"))
            .count(),
        1
    );
    let jobs = employment(&data, "Northwind Labs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(
        data.claim_sources(jobs[0]),
        vec![Origin::Linkedin, Origin::Resume]
    );
    assert_eq!(data.education.len(), 1);
}

#[tokio::test]
async fn conflicting_dates_stay_reviewable_and_a_different_title_is_a_different_position() {
    let repo = MemoryProfiles::default();
    with_resume(&repo).await;
    let mut parsed = linkedin();
    parsed.experiences[0] = northwind_linkedin("2020-06");
    let (report, data) = import_linkedin(&repo, "v1", &parsed, 2).await;
    assert_eq!(report.conflicts, 1);
    assert_eq!(
        data.experiences
            .iter()
            .filter(|e| e.company.as_deref() == Some("Northwind Labs"))
            .count(),
        1,
        "same company and title: the same position"
    );
    let jobs = employment(&data, "Northwind Labs");
    assert_eq!(
        jobs.len(),
        2,
        "the resume's statement and LinkedIn's differing one"
    );
    let theirs = claim(
        &data,
        "LinkedIn export: Senior Software Engineer at Northwind Labs (Jun 2020 – Present)",
    );
    assert_eq!(theirs.confidence, Confidence::Medium);
    assert!(data.standing(theirs).needs_review());
    assert_eq!(
        theirs.basis.as_deref(),
        Some(
            "dates differ from your resume: “Senior Software Engineer at Northwind Labs (Mar 2021 – Present)”"
        )
    );
    assert_eq!(
        data.describe(theirs),
        "read from your LinkedIn export, which disagrees with another source; check it"
    );
    let ours = claim(
        &data,
        "Senior Software Engineer at Northwind Labs (Mar 2021 – Present)",
    );
    assert!(
        ours.corroborations.is_empty(),
        "no silent merge of different dates"
    );

    // Re-importing does not duplicate the conflict.
    let (again, data) = import_linkedin(&repo, "v1", &parsed, 3).await;
    assert_eq!(again.claims.added, 0);
    assert_eq!(employment(&data, "Northwind Labs").len(), 2);

    // A different title at a company another source has is kept apart.
    let mut other = linkedin();
    other.experiences[0].title = Some("Staff Software Engineer".into());
    other.experiences[0].header =
        "Staff Software Engineer · Northwind Labs · 2021-03 – Present".into();
    let repo = MemoryProfiles::default();
    with_resume(&repo).await;
    let (_, data) = import_linkedin(&repo, "v2", &other, 2).await;
    assert_eq!(
        data.experiences
            .iter()
            .filter(|e| e.company.as_deref() == Some("Northwind Labs"))
            .count(),
        2
    );
}

#[tokio::test]
async fn ambiguous_matches_stay_separate() {
    let repo = MemoryProfiles::default();
    // The user entered the same position twice (say, two stints without dates).
    let service = ProfileService::new(&repo);
    for _ in 0..2 {
        service
            .add_experience(
                ExperienceEdit {
                    company: Some(Some("Contoso Freight".into())),
                    title: Some(Some("Software Engineer".into())),
                    ..ExperienceEdit::default()
                },
                at(1),
            )
            .await
            .unwrap();
    }
    let (report, data) = import_linkedin(&repo, "v1", &linkedin(), 2).await;
    assert!(
        report
            .notes
            .iter()
            .any(|n| n.contains("matches 2 positions already in the profile; kept separate")),
        "{:?}",
        report.notes
    );
    assert_eq!(
        data.experiences
            .iter()
            .filter(|e| e.company.as_deref() == Some("Contoso Freight"))
            .count(),
        3
    );
}

#[tokio::test]
async fn user_decisions_stay_authoritative() {
    let repo = MemoryProfiles::default();
    let data = with_resume(&repo).await;
    let service = ProfileService::new(&repo);
    let job = claim(
        &data,
        "Senior Software Engineer at Northwind Labs (Mar 2021 – Present)",
    )
    .id;
    let bullet = claim(&data, "Built the settlement pipeline in Rust.").id;
    let edu = data.education[0].id;
    service
        .decide_claims(&[job.to_string()], Verification::Confirmed, None, at(2))
        .await
        .unwrap();
    service
        .decide_claims(&[bullet.to_string()], Verification::Rejected, None, at(2))
        .await
        .unwrap();
    service.remove(&edu.to_string(), at(2)).await.unwrap();

    let (report, data) = import_linkedin(&repo, "v1", &linkedin(), 3).await;
    let job = data.claim(job).unwrap();
    assert_eq!(
        job.verification,
        Verification::Confirmed,
        "confirmation kept"
    );
    assert!(data.standing(job).is_usable());
    let bullet = data.claim(bullet).unwrap();
    assert_eq!(bullet.verification, Verification::Rejected);
    assert!(!data.standing(bullet).is_usable(), "not resurrected");
    assert_eq!(
        data.claims
            .iter()
            .filter(|c| c.text == "Built the settlement pipeline in Rust.")
            .count(),
        1
    );
    assert_eq!(report.kept_rejected, 1);
    // The rejected degree is not brought back as a new record.
    assert_eq!(data.education.len(), 1);
    assert!(data.education[0].meta.is_rejected());
    assert!(data.visible_education().is_empty());
}

#[tokio::test]
async fn the_users_own_position_gains_evidence_and_stays_theirs() {
    let repo = MemoryProfiles::default();
    let service = ProfileService::new(&repo);
    let added = service
        .add_experience(
            ExperienceEdit {
                company: Some(Some("Contoso Freight".into())),
                title: Some(Some("Software Engineer".into())),
                start: Some(d("2018-01")),
                end: Some(d("2021-02")),
                ..ExperienceEdit::default()
            },
            at(1),
        )
        .await
        .unwrap();
    let (report, data) = import_linkedin(&repo, "v1", &linkedin(), 2).await;
    assert_eq!(report.experiences.corroborated, 1);
    let contoso: Vec<&Experience> = data
        .experiences
        .iter()
        .filter(|e| e.company.as_deref() == Some("Contoso Freight"))
        .collect();
    assert_eq!(contoso.len(), 1);
    assert_eq!(contoso[0].id, added.id);
    assert_eq!(contoso[0].meta.origin, Origin::User);
    let jobs = employment(&data, "Contoso Freight");
    assert_eq!(
        jobs.len(),
        1,
        "the user's statement, now with LinkedIn behind it"
    );
    assert_eq!(jobs[0].provenance, Provenance::UserEntered);
    assert_eq!(data.claim_sources(jobs[0]), vec![Origin::Linkedin]);

    // LinkedIn dropping it takes only LinkedIn's evidence away.
    let mut without = linkedin();
    without.experiences.truncate(1);
    let (_, data) = import_linkedin(&repo, "v2", &without, 3).await;
    let e = data.experience(added.id).unwrap();
    assert!(!e.meta.is_stale());
    assert!(e.meta.corroborations.is_empty());
    let jobs = employment(&data, "Contoso Freight");
    assert!(jobs[0].corroborations.is_empty());
    assert!(data.standing(jobs[0]).is_usable());
}

#[tokio::test]
async fn staleness_needs_every_source_to_drop_it() {
    let repo = MemoryProfiles::default();
    with_resume(&repo).await;
    import_linkedin(&repo, "v1", &linkedin(), 2).await;

    // The resume drops Northwind; LinkedIn still lists it.
    let mut shorter = resume();
    shorter.experiences.clear();
    let (report, data) = ProfileService::new(&repo)
        .import_resume(resume_doc("resume-v2"), &shorter, at(3))
        .await
        .unwrap();
    assert_eq!(report.experiences.stale, 0);
    let northwind = data
        .experiences
        .iter()
        .find(|e| e.company.as_deref() == Some("Northwind Labs"))
        .unwrap();
    assert!(!northwind.meta.is_stale());
    assert_eq!(data.record_sources(&northwind.meta), vec![Origin::Linkedin]);
    let job = employment(&data, "Northwind Labs")[0];
    assert!(job.stale_since.is_none());
    assert!(data.standing(job).is_usable());
    assert_eq!(data.describe(job), "quoted from your LinkedIn export");

    // Now LinkedIn drops it too: stale, and the confirmation needs renewing.
    ProfileService::new(&repo)
        .decide_claims(&[job.id.to_string()], Verification::Confirmed, None, at(4))
        .await
        .unwrap();
    let mut without = linkedin();
    without.experiences.remove(0);
    let (report, data) = import_linkedin(&repo, "v2", &without, 5).await;
    assert_eq!(report.experiences.stale, 1);
    assert_eq!(report.stale_confirmed, 1);
    let job = employment(&data, "Northwind Labs")[0];
    assert!(job.stale_since.is_some());
    assert_eq!(
        job.verification,
        Verification::Confirmed,
        "the decision is kept"
    );
    assert_eq!(
        data.standing(job),
        Standing::NeedsReview(ReviewReason::SourceRemoved)
    );
    assert_eq!(
        data.describe(job),
        "no longer in your LinkedIn export; confirm to keep using it"
    );
}

#[tokio::test]
async fn missing_categories_are_not_negative_evidence() {
    let repo = MemoryProfiles::default();
    let before = with_resume(&repo).await;
    // An export with positions only: no skills, education or certifications.
    let only_positions = ParsedResume {
        experiences: vec![northwind_linkedin("2021-03")],
        ..ParsedResume::default()
    };
    let (report, data) = import_linkedin(&repo, "v1", &only_positions, 2).await;
    assert_eq!(report.skills.stale, 0);
    assert_eq!(report.education.stale, 0);
    assert_eq!(report.claims.stale, 0);
    assert_eq!(data.skills.len(), before.skills.len());
    assert!(data.skills.iter().all(|s| !s.meta.is_stale()));
    assert!(!data.education[0].meta.is_stale());
    assert!(
        !data
            .claims
            .iter()
            .any(|c| c.text.to_lowercase().contains("no certification")),
        "absence is never turned into a claim"
    );
}

#[tokio::test]
async fn removing_linkedin_keeps_what_other_sources_and_decisions_support() {
    let repo = MemoryProfiles::default();
    with_resume(&repo).await;
    let (_, data) = import_linkedin(&repo, "v1", &linkedin(), 2).await;
    let service = ProfileService::new(&repo);
    let cert = claim(
        &data,
        "Certifications: Certified Kafka Developer — Example Institute (Jan 2024)",
    )
    .id;
    service
        .decide_claims(&[cert.to_string()], Verification::Confirmed, None, at(3))
        .await
        .unwrap();

    let (removal, data) = service
        .remove_source(Origin::Linkedin, at(4))
        .await
        .unwrap();
    assert_eq!(removal.documents, 1);
    assert!(
        data.documents
            .iter()
            .all(|d| d.kind != DocumentKind::Linkedin)
    );
    // Northwind (resume + LinkedIn) stays, back to the resume alone.
    let job = employment(&data, "Northwind Labs")[0];
    assert!(job.corroborations.is_empty());
    assert_eq!(data.claim_sources(job), vec![Origin::Resume]);
    // Contoso (LinkedIn only, never reviewed) is gone with its claims.
    assert!(
        data.experiences
            .iter()
            .all(|e| e.company.as_deref() != Some("Contoso Freight"))
    );
    assert!(employment(&data, "Contoso Freight").is_empty());
    // The confirmed certification is kept, without a source, for review.
    let cert = data.claim(cert).unwrap();
    assert_eq!(cert.source, None);
    assert_eq!(
        data.standing(cert),
        Standing::NeedsReview(ReviewReason::SourceRemoved)
    );
    assert_eq!(
        data.describe(cert),
        "its source was removed; confirm to keep using it"
    );
    assert_eq!(removal.decisions_kept, 1);
    // Kafka was only LinkedIn's: unevidenced now.
    assert!(
        data.skills
            .iter()
            .filter(|s| s.name == "Kafka")
            .all(|s| s.meta.is_stale())
    );
    // Rust is still the resume's.
    assert!(
        data.skills
            .iter()
            .any(|s| s.name == "Rust" && !s.meta.is_stale())
    );
    // Nothing points at the removed document; the profile exports cleanly.
    ProfileExport::from_data(&data, at(5), None)
        .validate()
        .unwrap();
    assert!(
        service
            .remove_source(Origin::Linkedin, at(6))
            .await
            .is_err(),
        "nothing left to remove"
    );
    assert!(service.remove_source(Origin::Resume, at(6)).await.is_err());

    // Importing it again brings LinkedIn's evidence back.
    let (report, data) = import_linkedin(&repo, "v1", &linkedin(), 7).await;
    assert!(report.first_import);
    assert_eq!(employment(&data, "Contoso Freight").len(), 1);
    assert_eq!(
        data.claim_sources(employment(&data, "Northwind Labs")[0]),
        vec![Origin::Resume, Origin::Linkedin]
    );
}

// --- GitHub ---

fn gh_repo(id: u64, name: &str, language: &str, pushed: DateTime<Utc>) -> GithubRepo {
    GithubRepo {
        id,
        name: name.into(),
        full_name: format!("rileyx/{name}"),
        owner: "rileyx".into(),
        html_url: format!("https://github.com/rileyx/{name}"),
        description: None,
        fork: false,
        archived: false,
        is_template: false,
        private: false,
        size: 120,
        stars: 3,
        forks: 0,
        language: Some(language.into()),
        languages: Some(vec![(language.into(), 9_000), ("Shell".into(), 1_000)]),
        topics: Vec::new(),
        created_at: Some(Utc.with_ymd_and_hms(2022, 1, 10, 0, 0, 0).unwrap()),
        pushed_at: Some(pushed),
    }
}

fn snapshot() -> GithubSnapshot {
    let recent = Utc.with_ymd_and_hms(2026, 8, 1, 0, 0, 0).unwrap();
    let old = Utc.with_ymd_and_hms(2023, 2, 1, 0, 0, 0).unwrap();
    let mut lox = gh_repo(1, "lox", "Rust", recent);
    lox.description = Some("A tiny interpreter for payment rules".into());
    lox.topics = vec!["interpreter".into(), "payments".into()];
    lox.stars = 42;
    let mut archived = gh_repo(2, "oldcli", "Go", old);
    archived.archived = true;
    let mut fork = gh_repo(3, "tokio", "Rust", recent);
    fork.fork = true;
    let mut empty = gh_repo(4, "empty", "Rust", recent);
    empty.size = 0;
    let mut org = gh_repo(5, "platform", "Go", recent);
    org.owner = "northwind-labs".into();
    org.full_name = "northwind-labs/platform".into();
    let readme = gh_repo(6, "rileyx", "Markdown", recent);
    GithubSnapshot {
        account: GithubAccount {
            login: "rileyx".into(),
            html_url: "https://github.com/rileyx".into(),
            kind: "User".into(),
            public_repos: 6,
            created_at: Some(Utc.with_ymd_and_hms(2015, 3, 1, 0, 0, 0).unwrap()),
        },
        repos: vec![lox, archived, fork, empty, org, readme],
        orgs: vec!["northwind-labs".into()],
        fetched_at: at(0),
        problems: Vec::new(),
    }
}

#[tokio::test]
async fn github_evidence_is_about_public_code_not_expertise() {
    let repo = MemoryProfiles::default();
    let (import, data) = ProfileService::new(&repo)
        .import_github(&snapshot(), at(1))
        .await
        .unwrap();
    let s = &import.selection;
    assert_eq!(s.kept, vec!["rileyx/lox", "rileyx/oldcli"]);
    assert_eq!((s.forks, s.empty, s.not_owned, s.archived), (1, 2, 1, 1));
    assert_eq!(s.orgs, 1);
    assert_eq!(s.languages, vec!["Go", "Rust"]);
    assert_eq!(data.projects.len(), 2);
    assert!(
        data.experiences.is_empty(),
        "an organization is not employment"
    );
    let doc = &data.documents[0];
    assert_eq!(doc.kind, DocumentKind::Github);
    assert_eq!(doc.file_name.as_deref(), Some("github.com/rileyx"));
    assert!(
        doc.text
            .contains("Public organizations (membership, not employment): northwind-labs")
    );
    assert!(doc.text.contains(
        "Not used as evidence: 1 fork, 2 empty repositories, 1 repository owned by others."
    ));

    // Facts: ownership and main languages (Shell is 10%: not a main one).
    let owns = claim(&data, "Owns the public GitHub repository rileyx/lox");
    assert!(data.standing(owns).is_usable());
    assert_eq!(data.describe(owns), "read from GitHub's public API");
    claim(
        &data,
        "Owns the public GitHub repository rileyx/oldcli (archived)",
    );
    let rust = claim(&data, "Rust code in rileyx/lox (GitHub)");
    assert_eq!(rust.provenance, Provenance::Extracted);
    assert!(
        rust.source
            .as_ref()
            .unwrap()
            .snippet
            .starts_with("Languages: Rust 90%")
    );
    assert!(!texts(&data).iter().any(|t| t.contains("Shell")));
    // The one conclusion is an inference, for review; old work is not "recent".
    let recent = claim(
        &data,
        "Recent hands-on Rust work in public GitHub repositories",
    );
    assert_eq!(recent.provenance, Provenance::Inferred);
    assert!(data.standing(recent).needs_review());
    assert!(recent.basis.as_deref().unwrap().contains("rileyx/lox"));
    assert!(
        !texts(&data)
            .iter()
            .any(|t| t.contains("Recent hands-on Go"))
    );
    // Topics suggest a domain: inferred, never a fact.
    let domain = claim(&data, "Public project about payments: rileyx/lox");
    assert_eq!(domain.provenance, Provenance::Inferred);
    // No overclaiming anywhere.
    for c in &data.claims {
        let lower = c.text.to_lowercase();
        for word in [
            "expert",
            "experienced",
            "senior",
            "proficient",
            "skilled",
            "stars",
        ] {
            assert!(!lower.contains(word), "{:?} says {word}", c.text);
        }
        assert!(
            !matches!(
                c.kind,
                ClaimKind::Role | ClaimKind::Ownership | ClaimKind::Employment
            ),
            "{:?}",
            c.text
        );
    }
    // A project with Rust code is demonstrated use, for ranking.
    let skill = data.skills.iter().find(|s| s.name == "Rust").unwrap();
    assert_eq!(skill.meta.origin, Origin::Github);
    assert_eq!(
        data.skill_evidence(skill).strength,
        EvidenceStrength::Demonstrated
    );
}

#[tokio::test]
async fn github_reimport_is_idempotent_and_follows_changes() {
    let repo = MemoryProfiles::default();
    let service = ProfileService::new(&repo);
    let (_, before) = service.import_github(&snapshot(), at(1)).await.unwrap();
    let (again, after) = service.import_github(&snapshot(), at(2)).await.unwrap();
    assert!(again.report.same_file);
    assert_eq!(again.report.projects.unchanged, 2);
    assert_eq!(again.report.claims.added + again.report.claims.updated, 0);
    assert_eq!(ids(&before), ids(&after));
    assert_eq!(after.documents.len(), 1);

    // Stars and pushes change: evidence refreshed, no claim churn.
    let lox_owns = claim(&after, "Owns the public GitHub repository rileyx/lox").id;
    service
        .decide_claims(
            &[lox_owns.to_string()],
            Verification::Confirmed,
            None,
            at(3),
        )
        .await
        .unwrap();
    let mut changed = snapshot();
    changed.repos[0].stars = 99;
    changed.repos[0].pushed_at = Some(Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap());
    // oldcli is deleted; a new repository appears; lox's language stats
    // could not be read this time.
    changed.repos.remove(1);
    changed.repos.push(gh_repo(
        7,
        "ledger",
        "Go",
        Utc.with_ymd_and_hms(2026, 7, 1, 0, 0, 0).unwrap(),
    ));
    changed.repos[0].languages = None;
    changed.problems = vec!["languages of rileyx/lox: HTTP 502".into()];
    let (report, data) = service.import_github(&changed, at(4)).await.unwrap();
    assert!(!report.report.same_file);
    assert_eq!(report.report.projects.added, 1);
    assert_eq!(report.report.projects.stale, 1);
    assert!(report.report.notes.iter().any(|n| n.contains("HTTP 502")));
    let owns = data.claim(lox_owns).unwrap();
    assert_eq!(
        owns.verification,
        Verification::Confirmed,
        "not reset by a push"
    );
    assert!(data.standing(owns).is_usable());
    // Without statistics, the primary language still counts.
    assert!(
        claim(&data, "Rust code in rileyx/lox (GitHub)")
            .stale_since
            .is_none()
    );
    let oldcli = data
        .projects
        .iter()
        .find(|p| p.name == "rileyx/oldcli")
        .unwrap();
    assert!(
        oldcli.meta.is_stale(),
        "a deleted repository is stale, not gone"
    );
    assert!(
        claim(
            &data,
            "Recent hands-on Go work in public GitHub repositories"
        )
        .basis
        .is_some()
    );
}

#[tokio::test]
async fn a_profile_follows_one_github_account() {
    let repo = MemoryProfiles::default();
    let service = ProfileService::new(&repo);
    service.import_github(&snapshot(), at(1)).await.unwrap();
    let mut other = snapshot();
    other.account.login = "someone-else".into();
    let err = service.import_github(&other, at(2)).await.unwrap_err();
    assert!(err.to_string().contains("remove-source github"), "{err}");
    // Case does not make a different account.
    let mut same = snapshot();
    same.account.login = "RileyX".into();
    for r in &mut same.repos {
        if r.owner == "rileyx" {
            r.owner = "RileyX".into();
        }
    }
    service.import_github(&same, at(3)).await.unwrap();
}

#[tokio::test]
async fn resume_linkedin_and_github_meet_in_one_graph() {
    let repo = MemoryProfiles::default();
    let service = ProfileService::new(&repo);
    // The resume lists the lox project with its URL.
    let mut parsed = resume();
    parsed.projects.push(ParsedProject {
        name: "lox".into(),
        url: Some("https://github.com/rileyx/lox".into()),
        header: "lox — https://github.com/rileyx/lox".into(),
        tech_line: Some("Tech: Rust".into()),
        technologies: vec!["Rust".into()],
        ..ParsedProject::default()
    });
    service
        .import_resume(resume_doc("resume"), &parsed, at(1))
        .await
        .unwrap();
    import_linkedin(&repo, "v1", &linkedin(), 2).await;
    let (import, data) = service.import_github(&snapshot(), at(3)).await.unwrap();

    // lox: one project, resume + GitHub.
    assert_eq!(import.report.projects.corroborated, 1);
    let lox: Vec<&Project> = data
        .projects
        .iter()
        .filter(|p| p.name.ends_with("lox"))
        .collect();
    assert_eq!(lox.len(), 1);
    assert_eq!(lox[0].name, "lox", "the resume owns its fields");
    assert_eq!(
        data.record_sources(&lox[0].meta),
        vec![Origin::Resume, Origin::Github]
    );
    // The resume's "Used Rust at lox" and GitHub's code evidence: one claim.
    let used = claim(&data, "Used Rust at lox");
    assert_eq!(
        data.claim_sources(used),
        vec![Origin::Resume, Origin::Github]
    );
    // Related but different: the repository fact is its own claim.
    let owns = claim(&data, "Owns the public GitHub repository rileyx/lox");
    assert_eq!(owns.subject, Subject::Project(lox[0].id));
    // LinkedIn's listed skill and GitHub's code are different claims about
    // one skill.
    let rust = data.skills.iter().filter(|s| s.key == "rust").count();
    assert_eq!(rust, 1);
    let skill = data.skills.iter().find(|s| s.key == "rust").unwrap();
    let evidence = data.skill_evidence(skill);
    let kinds: Vec<ClaimKind> = evidence.claims.iter().map(|c| c.kind).collect();
    assert!(kinds.contains(&ClaimKind::Skill) && kinds.contains(&ClaimKind::Technology));
    // Removing GitHub keeps the resume project and its claims.
    let (removal, data) = service.remove_source(Origin::Github, at(4)).await.unwrap();
    assert!(removal.records_deleted >= 1, "oldcli was GitHub's alone");
    assert!(
        data.projects
            .iter()
            .any(|p| p.name == "lox" && !p.meta.is_stale())
    );
    assert!(data.projects.iter().all(|p| p.name != "rileyx/oldcli"));
    assert_eq!(
        data.claim_sources(claim(&data, "Used Rust at lox")),
        vec![Origin::Resume]
    );
    assert!(!texts(&data).iter().any(|t| t.contains("GitHub")));
    ProfileExport::from_data(&data, at(5), None)
        .validate()
        .unwrap();
}
