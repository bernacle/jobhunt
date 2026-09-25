//! Use-case tests through [`ProfileService`] and the in-memory repository:
//! resume re-import semantics, the evidence policy, preferences, and the
//! export format.

use chrono::{DateTime, TimeZone, Utc};

use crate::memory::MemoryProfiles;
use crate::*;

fn at(minute: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, minute, 0).unwrap()
}

fn d(s: &str) -> Option<PartialDate> {
    Some(s.parse().unwrap())
}

fn document(name: &str) -> SourceDocument {
    let hex = jobhunt_core::StableId::derive("test.sha", &[name]).to_hex();
    SourceDocument {
        id: DocumentId::derive(&[name]),
        kind: DocumentKind::Text,
        file_name: Some(format!("{name}.txt")),
        sha256: format!("{hex}{hex}"),
        pages: Some(1),
        text: format!("text of {name}"),
        parser: "test".into(),
        first_imported_at: at(0),
        last_imported_at: at(0),
    }
}

fn ledgerly_entry(title: &str, bullets: &[&str]) -> ParsedExperience {
    ParsedExperience {
        company: Some("Ledgerly".into()),
        title: Some(title.into()),
        start: d("2022-03"),
        current: true,
        header: format!("Ledgerly — {title}  Mar 2022 – Present"),
        bullets: bullets.iter().map(|b| (*b).to_owned()).collect(),
        tech_line: Some("Tech: Rust, Kubernetes".into()),
        technologies: vec!["Rust".into(), "Kubernetes".into()],
        ..ParsedExperience::default()
    }
}

fn bank(title: &str, start: &str, end: &str) -> ParsedExperience {
    ParsedExperience {
        company: Some("Banco Horizonte".into()),
        title: Some(title.into()),
        start: d(start),
        end: d(end),
        header: format!("Banco Horizonte\n{title}  {start} – {end}"),
        bullets: vec!["Maintained the card authorization service.".into()],
        ..ParsedExperience::default()
    }
}

const RULES: &str =
    "Designed a rules engine for Travel Rule compliance checks, reducing manual reviews by 40%.";
const PAYMENTS: &str = "Integrated four payment providers behind a single settlement API.";
const MENTORED: &str = "Mentored three engineers.";

fn v1() -> ParsedResume {
    ParsedResume {
        basics: ParsedBasics {
            name: Some("Marina Costa".into()),
            headline: Some("Backend engineer".into()),
            ..ParsedBasics::default()
        },
        experiences: vec![
            ledgerly_entry("Senior Software Engineer", &[RULES, PAYMENTS, MENTORED]),
            bank("Software Engineer II", "2020-01", "2022-04"),
            bank("Software Engineer", "2018-06", "2019-12"),
            ParsedExperience {
                title: Some("Full Stack Developer".into()),
                employment: Some(EmploymentKind::Freelance),
                header: "Freelance — Full Stack Developer".into(),
                bullets: vec!["Built web applications with React and Node.js.".into()],
                notes: vec!["no dates found".into()],
                ..ParsedExperience::default()
            },
        ],
        education: vec![ParsedEducation {
            institution: "Universidade de São Paulo".into(),
            degree: Some("B.Sc.".into()),
            field: Some("Computer Science".into()),
            start: d("2014"),
            end: d("2018"),
            header: "Universidade de São Paulo — B.Sc. in Computer Science  2014 – 2018".into(),
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

fn find<'a>(data: &'a ProfileData, text: &str) -> &'a Claim {
    data.claims
        .iter()
        .find(|c| c.text == text)
        .unwrap_or_else(|| panic!("no claim {text:?}"))
}

fn experience<'a>(data: &'a ProfileData, title: &str) -> &'a Experience {
    data.experiences
        .iter()
        .find(|e| e.title.as_deref() == Some(title))
        .unwrap_or_else(|| panic!("no experience {title:?}"))
}

async fn imported(repo: &MemoryProfiles) -> ProfileData {
    let service = ProfileService::new(repo);
    let (report, data) = service
        .import_resume(document("v1"), &v1(), at(1))
        .await
        .unwrap();
    assert!(report.first_import);
    assert_eq!(report.experiences.added, 4);
    data
}

#[tokio::test]
async fn first_import_builds_records_and_grounded_claims() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    assert_eq!(data.profile.name.as_deref(), Some("Marina Costa"));
    assert_eq!(data.experiences.len(), 4);
    assert_eq!(data.education.len(), 1);

    // A direct resume claim: verbatim snippet, extracted, usable.
    let rules = find(&data, RULES);
    assert_eq!(rules.kind, ClaimKind::Accomplishment);
    assert_eq!(rules.provenance, Provenance::Extracted);
    assert_eq!(
        rules.source.as_ref().unwrap().snippet,
        RULES,
        "the resume's own words"
    );
    assert_eq!(
        rules.source.as_ref().unwrap().section.as_deref(),
        Some("Experience")
    );
    assert_eq!(
        data.standing(rules),
        Standing::Usable(UsableBecause::Grounded)
    );
    assert_eq!(find(&data, MENTORED).kind, ClaimKind::Responsibility);

    // Inferred claims carry their basis and need review.
    let ledgerly = experience(&data, "Senior Software Engineer").id;
    let domain = data
        .claims
        .iter()
        .find(|c| c.kind == ClaimKind::Domain && c.topic.as_deref() == Some("payments"))
        .unwrap();
    assert_eq!(domain.subject, Subject::Experience(ledgerly));
    assert_eq!(domain.provenance, Provenance::Inferred);
    assert!(domain.basis.as_deref().unwrap().contains("payment"));
    assert_eq!(
        data.standing(domain),
        Standing::NeedsReview(ReviewReason::Inferred)
    );
    assert_eq!(data.experiences_in_domain("payments")[0].id, ledgerly);
    assert!(
        data.experiences_in_domain("banking")
            .iter()
            .all(|e| e.company.as_deref() == Some("Banco Horizonte"))
    );

    // Role and seniority signals exist, with reasons.
    let roles: Vec<String> = data
        .signals(ClaimKind::Role)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(roles.contains(&"full stack".to_owned()), "{roles:?}");
    let ownership: Vec<String> = data
        .signals(ClaimKind::Ownership)
        .into_iter()
        .map(|(t, _)| t)
        .collect();
    assert!(
        ownership.contains(&"senior".to_owned()) && ownership.contains(&"mentorship".to_owned())
    );

    // Skills: used (demonstrated) versus only listed.
    let evidence = data.skills_with_evidence();
    let strength = |name: &str| {
        evidence
            .iter()
            .find(|s| s.skill.name == name)
            .unwrap()
            .strength
    };
    assert_eq!(strength("Rust"), EvidenceStrength::Demonstrated);
    assert_eq!(strength("Go"), EvidenceStrength::Listed);
    let rust = evidence.iter().find(|s| s.skill.name == "Rust").unwrap();
    assert_eq!(rust.last_seen, Some(LastSeen::Current));
    let techs: Vec<&str> = data
        .technologies_with_evidence()
        .iter()
        .map(|s| s.skill.name.as_str())
        .collect();
    assert!(techs.contains(&"Kubernetes") && !techs.contains(&"Go"));

    // Undated freelance work stays undated and is reported.
    let freelance = experience(&data, "Full Stack Developer");
    assert_eq!((freelance.start, freelance.end), (None, None));
    assert!(
        data.gaps()
            .iter()
            .any(|g| g.message.contains("no dates found"))
    );
}

#[tokio::test]
async fn importing_the_same_resume_again_changes_nothing() {
    let repo = MemoryProfiles::default();
    let before = imported(&repo).await;
    let service = ProfileService::new(&repo);
    let (report, after) = service
        .import_resume(document("v1"), &v1(), at(2))
        .await
        .unwrap();
    assert!(report.same_file && !report.first_import);
    assert_eq!(
        report.experiences,
        Tally {
            unchanged: 4,
            ..Tally::default()
        }
    );
    assert_eq!(report.claims.added, 0);
    assert_eq!(report.claims.updated, 0);
    assert_eq!(report.claims.stale, 0);
    assert_eq!(after.experiences.len(), before.experiences.len());
    assert_eq!(after.claims.len(), before.claims.len());
    assert_eq!(after.skills.len(), before.skills.len());
    assert_eq!(after.documents.len(), 1);
    let ids = |d: &ProfileData| {
        let mut ids: Vec<String> = d.claims.iter().map(|c| c.id.to_string()).collect();
        ids.sort();
        ids
    };
    assert_eq!(ids(&before), ids(&after), "stable ids");
}

#[tokio::test]
async fn decisions_and_edits_survive_a_changed_resume() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    let service = ProfileService::new(&repo);
    let payments = find(&data, PAYMENTS).id;
    let rules = find(&data, RULES).id;
    let mentored = find(&data, MENTORED).id;
    let ledgerly = experience(&data, "Senior Software Engineer").id;
    let domain = data
        .claims
        .iter()
        .find(|c| c.kind == ClaimKind::Domain && c.topic.as_deref() == Some("payments"))
        .unwrap()
        .id;
    let bank2 = experience(&data, "Software Engineer II").id;
    let banking = data
        .claims
        .iter()
        .find(|c| c.topic.as_deref() == Some("banking") && c.subject == Subject::Experience(bank2))
        .unwrap()
        .id;
    let full_stack = data
        .claims
        .iter()
        .find(|c| c.topic.as_deref() == Some("full stack"))
        .unwrap()
        .id;
    service
        .decide_claims(
            &[
                payments.to_string(),
                rules.to_string(),
                mentored.to_string(),
                domain.to_string(),
            ],
            Verification::Confirmed,
            None,
            at(2),
        )
        .await
        .unwrap();
    service
        .decide_claims(
            &[banking.to_string(), full_stack.to_string()],
            Verification::Rejected,
            Some("not me".into()),
            at(2),
        )
        .await
        .unwrap();
    // A manual correction and manual records.
    service
        .edit_experience(
            &bank2.to_string(),
            ExperienceEdit {
                location: Some(Some("São Paulo, Brazil (hybrid)".into())),
                ..ExperienceEdit::default()
            },
            at(3),
        )
        .await
        .unwrap();
    let manual = service
        .add_experience(
            ExperienceEdit {
                company: Some(Some("Open Source".into())),
                title: Some(Some("Maintainer".into())),
                start: Some(d("2021")),
                current: Some(true),
                technologies: vec!["Rust".into()],
                ..ExperienceEdit::default()
            },
            at(3),
        )
        .await
        .unwrap();
    let manual_claim = service
        .add_claim(
            "Gave a talk at RustConf",
            ClaimKind::Other,
            None,
            None,
            at(3),
        )
        .await
        .unwrap();
    service
        .add_statement("At least $120k. Avoid agencies.", &RuleParser, at(3))
        .await
        .unwrap();

    // v2: promoted title (same company and start), a reworded bullet, a
    // removed bullet, the freelance role gone, a changed location.
    let mut v2 = v1();
    v2.experiences[0] = ledgerly_entry(
        "Staff Software Engineer",
        &[
            "Designed a rules engine for Travel Rule compliance checks, reducing manual reviews by 55%.",
            PAYMENTS,
        ],
    );
    v2.experiences[1].location = Some("Remote".into());
    v2.experiences.truncate(3);
    let (report, after) = service
        .import_resume(document("v2"), &v2, at(4))
        .await
        .unwrap();

    // No blind duplication: the promoted role is the same record.
    assert_eq!(report.experiences.added, 0);
    assert_eq!(report.experiences.stale, 1, "the freelance role");
    let promoted = after.experience(ledgerly).unwrap();
    assert_eq!(
        promoted.title.as_deref(),
        Some("Staff Software Engineer"),
        "source facts update"
    );
    assert_eq!(after.experiences.len(), 5);
    assert!(experience(&after, "Full Stack Developer").meta.is_stale());

    // Confirmed claims whose source is still there stay confirmed.
    assert_eq!(
        after.claim(payments).unwrap().verification,
        Verification::Confirmed
    );
    assert_eq!(
        after.standing(after.claim(payments).unwrap()),
        Standing::Usable(UsableBecause::Confirmed)
    );
    assert_eq!(
        after.claim(domain).unwrap().verification,
        Verification::Confirmed
    );
    // Rejected stays rejected and is never usable, whether the resume
    // still says it or not.
    assert_eq!(
        after.claim(banking).unwrap().verification,
        Verification::Rejected
    );
    assert_eq!(
        after.claim(banking).unwrap().note.as_deref(),
        Some("not me")
    );
    assert_eq!(
        after.standing(after.claim(banking).unwrap()),
        Standing::Rejected
    );
    assert_eq!(report.kept_rejected, 1);
    let gone = after.claim(full_stack).unwrap();
    assert!(gone.stale_since.is_some());
    assert_eq!(after.standing(gone), Standing::Rejected);
    // A confirmed bullet that left the resume is stale and needs review.
    let old_rules = after.claim(rules).unwrap();
    assert!(old_rules.stale_since.is_some());
    assert_eq!(
        old_rules.verification,
        Verification::Confirmed,
        "the decision is kept"
    );
    assert_eq!(
        after.standing(old_rules),
        Standing::NeedsReview(ReviewReason::SourceRemoved)
    );
    assert!(after.claim(mentored).unwrap().stale_since.is_some());
    assert_eq!(report.stale_confirmed, 2);
    // The reworded bullet is new, unverified, and points at the old one.
    let new_rules = find(
        &after,
        "Designed a rules engine for Travel Rule compliance checks, reducing manual reviews by 55%.",
    );
    assert_eq!(new_rules.verification, Verification::Unverified);
    assert_eq!(new_rules.supersedes, Some(rules));
    // Seniority follows the title: staff is new, senior is stale.
    let topics: Vec<(&str, bool)> = after
        .claims
        .iter()
        .filter(|c| c.kind == ClaimKind::Ownership && c.subject == Subject::Experience(ledgerly))
        .map(|c| (c.topic.as_deref().unwrap(), c.stale_since.is_some()))
        .collect();
    assert!(
        topics.contains(&("staff", false)) && topics.contains(&("senior", true)),
        "{topics:?}"
    );

    // The user's edit wins over the resume's new location.
    let bank2_after = after.experience(bank2).unwrap();
    assert_eq!(
        bank2_after.location.as_deref(),
        Some("São Paulo, Brazil (hybrid)")
    );
    assert!(report.preserved_edits >= 1);
    // Manual records, claims and preferences are untouched.
    assert!(
        after
            .experience(manual.id)
            .is_some_and(|e| !e.meta.is_stale())
    );
    assert!(
        after
            .claim(manual_claim.id)
            .is_some_and(|c| c.stale_since.is_none())
    );
    assert_eq!(after.preferences().active().count(), 2);
    assert_eq!(after.statements.len(), 1);
    assert_eq!(after.documents.len(), 2);

    // Re-importing v1 restores what came back, still with the decisions.
    let (report, again) = service
        .import_resume(document("v1"), &v1(), at(5))
        .await
        .unwrap();
    assert_eq!(report.experiences.restored, 1);
    let rules_again = again.claim(rules).unwrap();
    assert!(rules_again.stale_since.is_none());
    assert_eq!(
        again.standing(rules_again),
        Standing::Usable(UsableBecause::Confirmed)
    );
}

#[tokio::test]
async fn a_corrected_title_keeps_the_record_and_manual_titles_win() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    let service = ProfileService::new(&repo);
    let bank1 = experience(&data, "Software Engineer").id;
    let employment = data
        .claims
        .iter()
        .find(|c| c.kind == ClaimKind::Employment && c.subject == Subject::Experience(bank1))
        .unwrap()
        .id;
    service
        .decide_claims(
            &[employment.to_string()],
            Verification::Confirmed,
            None,
            at(2),
        )
        .await
        .unwrap();

    // The resume fixes the title: same company and start date.
    let mut fixed = v1();
    fixed.experiences[2].title = Some("Software Engineer I".into());
    let (report, after) = service
        .import_resume(document("fixed"), &fixed, at(3))
        .await
        .unwrap();
    assert_eq!(report.experiences.added, 0);
    assert_eq!(
        after.experience(bank1).unwrap().title.as_deref(),
        Some("Software Engineer I")
    );
    // The confirmed statement changed, so it must be confirmed again.
    let claim = after.claim(employment).unwrap();
    assert_eq!(claim.verification, Verification::Unverified);
    assert!(
        claim
            .text
            .contains("Software Engineer I at Banco Horizonte")
    );
    assert!(
        claim
            .note
            .as_deref()
            .unwrap()
            .starts_with("You had confirmed")
    );
    assert_eq!(report.reconfirm, 1);

    // The user corrects the title by hand; the resume's title no longer wins.
    service
        .edit_experience(
            &bank1.to_string(),
            ExperienceEdit {
                title: Some(Some("Junior Software Engineer".into())),
                ..ExperienceEdit::default()
            },
            at(4),
        )
        .await
        .unwrap();
    let (_, after) = service
        .import_resume(document("fixed"), &fixed, at(5))
        .await
        .unwrap();
    let e = after.experience(bank1).unwrap();
    assert_eq!(e.title.as_deref(), Some("Junior Software Engineer"));
    assert!(e.meta.is_edited("title"));
    let claim = after.claim(employment).unwrap();
    assert!(claim.edited && claim.verification == Verification::Confirmed);
    assert!(
        claim
            .text
            .starts_with("Junior Software Engineer at Banco Horizonte")
    );
}

#[tokio::test]
async fn removed_imported_records_stay_rejected_across_reimports() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    let service = ProfileService::new(&repo);
    let freelance = experience(&data, "Full Stack Developer").id;
    assert!(matches!(
        service.remove(&freelance.to_string(), at(2)).await.unwrap(),
        Removal::Rejected(_)
    ));
    let (_, after) = service
        .import_resume(document("v1"), &v1(), at(3))
        .await
        .unwrap();
    let e = after.experience(freelance).unwrap();
    assert!(e.meta.is_rejected());
    assert!(
        after
            .visible_experiences()
            .iter()
            .all(|x| x.id != freelance)
    );
    // Its claims are excluded from use too.
    assert!(
        after
            .claims_about(Subject::Experience(freelance), &[])
            .iter()
            .all(|c| after.standing(c) == Standing::Rejected)
    );
    // A user-created record is deleted outright, with its claims.
    let manual = service
        .add_experience(
            ExperienceEdit {
                company: Some(Some("Side gig".into())),
                ..ExperienceEdit::default()
            },
            at(4),
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .remove(&manual.id.to_string()[..12], at(5))
            .await
            .unwrap(),
        Removal::Deleted(_)
    ));
    let data = service.require().await.unwrap();
    assert!(data.experience(manual.id).is_none());
    assert!(
        data.claims
            .iter()
            .all(|c| c.subject != Subject::Experience(manual.id))
    );
}

#[tokio::test]
async fn manual_claims_and_the_evidence_policy() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    let service = ProfileService::new(&repo);
    let ledgerly = experience(&data, "Senior Software Engineer").id;
    let claim = service
        .add_claim(
            "Ran the incident review program",
            ClaimKind::Responsibility,
            Some(&ledgerly.to_string()[..10]),
            None,
            at(2),
        )
        .await
        .unwrap();
    assert_eq!(claim.provenance, Provenance::UserEntered);
    assert_eq!(claim.subject, Subject::Experience(ledgerly));
    assert_eq!(
        claim.standing(),
        Standing::Usable(UsableBecause::UserEntered)
    );

    // Editing an extracted claim keeps the original wording in a note.
    let rules = find(&data, RULES).id;
    let edited = service
        .edit_claim(
            &rules.to_string(),
            "Designed the Travel Rule rules engine",
            at(3),
        )
        .await
        .unwrap();
    assert!(edited.edited);
    assert_eq!(
        edited.note.as_deref(),
        Some(&*format!("Originally: “{RULES}”"))
    );
    assert_eq!(
        edited.source.as_ref().unwrap().snippet,
        RULES,
        "provenance preserved"
    );
    let (_, after) = service
        .import_resume(document("v1"), &v1(), at(4))
        .await
        .unwrap();
    assert_eq!(
        after.claim(rules).unwrap().text,
        "Designed the Travel Rule rules engine"
    );

    // Resetting a decision puts an inference back in the queue.
    let queue_before = after.review_queue().len();
    let inferred = after.review_queue()[0].id;
    service
        .decide_claims(
            &[inferred.to_string()],
            Verification::Confirmed,
            None,
            at(5),
        )
        .await
        .unwrap();
    assert_eq!(
        service.require().await.unwrap().review_queue().len(),
        queue_before - 1
    );
    service
        .decide_claims(
            &[inferred.to_string()],
            Verification::Unverified,
            None,
            at(6),
        )
        .await
        .unwrap();
    assert_eq!(
        service.require().await.unwrap().review_queue().len(),
        queue_before
    );

    // Unknown and ambiguous ids are reported, not guessed.
    assert!(matches!(
        service
            .decide_claims(&["clm_zz".into()], Verification::Confirmed, None, at(7))
            .await,
        Err(ProfileError::NotFound { .. })
    ));
    assert!(matches!(
        service
            .decide_claims(&["clm_".into()], Verification::Confirmed, None, at(7))
            .await,
        Err(ProfileError::Ambiguous { .. })
    ));
}

#[tokio::test]
async fn preferences_statements_and_structured_values() {
    let repo = MemoryProfiles::default();
    let service = ProfileService::new(&repo);
    // Preferences work before any resume is imported.
    let outcome = service
        .add_statement(
            "I want small product teams and at least $120k. Avoid pure SRE roles. Something with a good vibe.",
            &RuleParser,
            at(1),
        )
        .await
        .unwrap();
    assert_eq!(
        outcome.statement.text,
        "I want small product teams and at least $120k. Avoid pure SRE roles. Something with a good vibe.",
        "kept verbatim"
    );
    assert_eq!(outcome.statement.reading, StatementReading::Partial);
    assert_eq!(outcome.statement.unparsed, ["Something with a good vibe"]);
    assert_eq!(outcome.preferences.len(), 4);
    assert!(
        outcome
            .preferences
            .iter()
            .all(|p| p.statement == Some(outcome.statement.id))
    );

    let data = service.require().await.unwrap();
    let view = data.preferences();
    assert_eq!(view.unwanted_roles(), ["sre"]);
    let comp = view.compensation();
    assert!(matches!(
        comp.minimum[0].value,
        PreferenceValue::Compensation {
            amount: 120_000,
            ..
        }
    ));
    assert!(
        view.company_traits(Stance::Wanted)
            .contains(&CompanyTrait::SmallTeam)
    );

    // A structured value replaces the one read from the statement.
    let (pref, replaced) = service
        .set_preference(
            PreferenceValue::Compensation {
                bound: CompensationBound::Minimum,
                amount: 140_000,
                currency: Some("USD".into()),
                period: PayPeriod::Year,
                arrangement: None,
            },
            Stance::Required,
            at(2),
        )
        .await
        .unwrap();
    assert_eq!(replaced.len(), 1);
    let data = service.require().await.unwrap();
    let minimum = data.preferences().compensation().minimum;
    assert_eq!(minimum.len(), 1);
    assert_eq!(minimum[0].id, pref.id);
    let old = data
        .preferences
        .iter()
        .find(|p| p.id == replaced[0].id)
        .unwrap();
    assert!(
        !old.active && old.superseded_by == Some(pref.id),
        "history is kept"
    );

    // An uncertain reading is kept, flagged, with the raw text.
    let outcome = service
        .add_statement("maybe fintech?", &RuleParser, at(3))
        .await
        .unwrap();
    assert_eq!(outcome.preferences[0].certainty, Certainty::Uncertain);
    assert_eq!(outcome.statement.text, "maybe fintech?");
    let outcome = service
        .add_statement("purple elephants", &RuleParser, at(4))
        .await
        .unwrap();
    assert_eq!(outcome.statement.reading, StatementReading::NotUnderstood);
    assert!(outcome.preferences.is_empty());

    // Removing a statement removes what was read from it and brings back
    // what it had replaced.
    let first = service
        .add_statement("Remote only.", &RuleParser, at(5))
        .await
        .unwrap();
    let second = service
        .add_statement("Hybrid is fine, remote preferred.", &RuleParser, at(6))
        .await
        .unwrap();
    let data = service.require().await.unwrap();
    assert_eq!(data.preferences().location().work_modes.len(), 2);
    let remote_now = data
        .preferences()
        .location()
        .work_modes
        .into_iter()
        .find(|p| {
            p.value
                == PreferenceValue::WorkMode {
                    mode: WorkMode::Remote,
                }
        })
        .unwrap()
        .statement;
    assert_eq!(remote_now, Some(second.statement.id));
    service
        .remove(&second.statement.id.to_string(), at(7))
        .await
        .unwrap();
    let data = service.require().await.unwrap();
    let modes = data.preferences().location().work_modes;
    assert_eq!(modes.len(), 1);
    assert_eq!(modes[0].statement, Some(first.statement.id));
    assert_eq!(modes[0].stance, Stance::Required);
}

#[tokio::test]
async fn export_round_trips_and_import_is_all_or_nothing() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    let service = ProfileService::new(&repo);
    let rules = find(&data, RULES).id;
    service
        .decide_claims(
            &[rules.to_string()],
            Verification::Rejected,
            Some("wrong".into()),
            at(2),
        )
        .await
        .unwrap();
    service
        .add_statement("No agencies.", &RuleParser, at(2))
        .await
        .unwrap();
    let export = service.export(at(3), Some("test".into())).await.unwrap();
    let json = export.to_json().unwrap();
    assert!(json.contains("\"format\": \"jobhunt.profile\""));
    assert!(json.contains("\"version\": 1"));

    // Into a fresh store: everything, including decisions, comes back.
    let other = MemoryProfiles::default();
    let other_service = ProfileService::new(&other);
    let parsed = ProfileExport::parse(&json).unwrap();
    let restored = other_service.import_export(parsed, at(4)).await.unwrap();
    let original = service.require().await.unwrap();
    assert_eq!(restored.claims, original.claims);
    assert_eq!(restored.experiences, original.experiences);
    assert_eq!(restored.preferences, original.preferences);
    assert_eq!(restored.statements, original.statements);
    assert_eq!(restored.documents, original.documents);
    assert_eq!(
        restored.claim(rules).unwrap().verification,
        Verification::Rejected
    );

    // Unsupported versions and other files are refused with a reason.
    let future = json.replacen("\"version\": 1", "\"version\": 2", 1);
    assert!(matches!(
        ProfileExport::parse(&future),
        Err(ExportError::Version { .. })
    ));
    assert!(matches!(
        ProfileExport::parse("{\"format\": \"x\"}"),
        Err(ExportError::Format { .. })
    ));
    assert!(matches!(
        ProfileExport::parse("not json"),
        Err(ExportError::Json(_))
    ));
    let unknown = json.replacen("\"format\"", "\"surprise\": 1, \"format\"", 1);
    assert!(matches!(
        ProfileExport::parse(&unknown),
        Err(ExportError::Schema(_))
    ));

    // A broken reference fails validation, and nothing is written.
    let mut broken = ProfileExport::parse(&json).unwrap();
    let experience = broken.experiences.remove(0).id;
    let problems = match broken.validate() {
        Err(ExportError::Invalid(problems)) => problems,
        other => panic!("expected invalid, got {other:?}"),
    };
    assert!(problems.iter().any(|p| p.contains(&experience.to_string())));
    let before = service.require().await.unwrap();
    assert!(service.import_export(broken, at(5)).await.is_err());
    assert_eq!(service.require().await.unwrap(), before, "unchanged");
}

#[tokio::test]
async fn concurrent_writers_do_not_overwrite_each_other() {
    let repo = MemoryProfiles::default();
    let data = imported(&repo).await;
    // Someone else saves first.
    let mut theirs = data.clone();
    theirs.profile.revision += 1;
    repo.save_profile(&theirs, data.profile.revision, &[])
        .await
        .unwrap();
    let mut mine = data.clone();
    mine.profile.revision += 1;
    let err = repo
        .save_profile(&mine, data.profile.revision, &[])
        .await
        .unwrap_err();
    assert!(matches!(err, StorageError::Conflict { .. }));
}
