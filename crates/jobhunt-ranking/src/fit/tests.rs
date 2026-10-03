//! Fit on synthetic postings and taste profiles: seniority both ways,
//! role depth, company shape, and how much each statement counts by whose
//! it is.

use jobhunt_profile::taste::{
    ComposedAssertion, TasteConfidence, TasteOrigin, TasteProfile, TasteReview, vocab,
};
use jobhunt_profile::{Polarity, TasteDimension, TasteId};

use super::*;
use crate::facets::{Level, facets};
use crate::testing::record;

/// One statement of a profile.
struct S {
    d: TasteDimension,
    v: &'static str,
    p: Polarity,
    origin: TasteOrigin,
    review: TasteReview,
    confidence: TasteConfidence,
}

/// The person's own statement.
fn said(d: TasteDimension, v: &'static str, p: Polarity) -> S {
    S {
        d,
        v,
        p,
        origin: TasteOrigin::Stated,
        review: TasteReview::Confirmed,
        confidence: TasteConfidence::High,
    }
}

fn read_from_words(d: TasteDimension, v: &'static str, p: Polarity, c: TasteConfidence) -> S {
    S {
        d,
        v,
        p,
        origin: TasteOrigin::Interpreted,
        review: TasteReview::Unreviewed,
        confidence: c,
    }
}

fn learned(d: TasteDimension, v: &'static str, p: Polarity, c: TasteConfidence) -> S {
    S {
        d,
        v,
        p,
        origin: TasteOrigin::Learned,
        review: TasteReview::Unreviewed,
        confidence: c,
    }
}

fn profile(statements: &[S]) -> TasteProfile {
    TasteProfile {
        assertions: statements
            .iter()
            .map(|s| ComposedAssertion {
                id: TasteId::derive(&[s.v, s.origin.as_str()]),
                dimension: s.d,
                value: s.v.to_owned(),
                polarity: s.p,
                text: vocab::sentence(s.d, s.v, s.p),
                confidence: s.confidence,
                origin: s.origin,
                review: s.review,
                sources: Vec::new(),
                against: Vec::new(),
                explanation: None,
                interpreter: None,
                original: None,
                stored: true,
            })
            .collect(),
        ..TasteProfile::default()
    }
}

fn senior_engineer() -> Person {
    Person {
        engineer: true,
        level: Some((Level::Senior, "Senior Software Engineer".into())),
        ..Person::default()
    }
}

fn fit(title: &str, description: &str, person: &Person, taste: &TasteProfile) -> FitAssessment {
    assess(
        &facets(&record("ashby:acme", title, description)),
        person,
        taste,
    )
}

use TasteDimension::{Company, Ownership, Seniority, Specialization, Team, WorkShape};

fn generalist() -> TasteProfile {
    profile(&[
        said(Seniority, "senior", Polarity::Prefer),
        said(Seniority, "early_career", Polarity::Avoid),
        said(WorkShape, "backend", Polarity::Prefer),
        said(Specialization, "deep", Polarity::Avoid),
        said(Team, "small_team", Polarity::Prefer),
        said(Ownership, "high", Polarity::Prefer),
    ])
}

const STARTUP_BACKEND: &str = "We are a 20-person startup. You'll join a team of 5 engineers \
    and own backend services end to end in Go and PostgreSQL.";

fn has_contradiction(f: &FitAssessment, kind: ContradictionKind) -> bool {
    f.contradictions
        .iter()
        .any(|c| c.kind == kind && c.severity == Severity::Material)
}

// ---------------------------------------------------------------------------
// Seniority.

#[test]
fn an_early_career_role_is_poor_for_a_senior_however_well_it_matches() {
    let f = fit(
        "Software Engineer, Early Career",
        STARTUP_BACKEND,
        &senior_engineer(),
        &generalist(),
    );
    assert_eq!(f.level, FitLevel::Poor, "{f:#?}");
    assert!(has_contradiction(&f, ContradictionKind::Seniority));
    assert!(f.contradictions[0].text.contains("early-career role"));
}

#[test]
fn a_senior_role_for_a_senior_is_affirmative() {
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &senior_engineer(),
        &generalist(),
    );
    assert_eq!(f.level, FitLevel::Strong, "{f:#?}");
    assert!(f.reasons.iter().any(|r| r.aspect == Aspect::Seniority));
}

#[test]
fn an_early_career_role_for_an_early_career_candidate() {
    let taste = profile(&[
        said(Seniority, "early_career", Polarity::Prefer),
        said(Seniority, "senior", Polarity::Avoid),
        said(WorkShape, "backend", Polarity::Prefer),
        said(TasteDimension::Culture, "mentorship", Polarity::Prefer),
    ]);
    let junior = Person {
        engineer: true,
        level: Some((Level::Junior, "Junior Software Engineer".into())),
        ..Person::default()
    };
    let early = fit(
        "Software Engineer, New Grad",
        "You'll ship backend features and APIs in TypeScript and Node.js alongside senior \
         engineers, with a dedicated mentor.",
        &junior,
        &taste,
    );
    assert_eq!(early.level, FitLevel::Strong, "{early:#?}");
    let senior = fit("Senior Backend Engineer", STARTUP_BACKEND, &junior, &taste);
    assert_eq!(senior.level, FitLevel::Poor);
    assert!(has_contradiction(&senior, ContradictionKind::Seniority));
}

#[test]
fn without_a_stated_level_the_latest_title_decides_two_steps_away() {
    let taste = profile(&[said(WorkShape, "backend", Polarity::Prefer)]);
    let f = fit("Junior Backend Engineer", "", &senior_engineer(), &taste);
    assert_eq!(f.level, FitLevel::Poor);
    assert!(f.contradictions[0].text.contains("latest title"), "{f:#?}");
}

#[test]
fn a_step_below_the_latest_title_is_noted_and_a_step_below_a_stated_level_holds() {
    let staff = Person {
        engineer: true,
        level: Some((Level::Staff, "Staff Software Engineer".into())),
        ..Person::default()
    };
    let taste = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Team, "small_team", Polarity::Prefer),
    ]);
    let f = fit("Senior Backend Engineer", STARTUP_BACKEND, &staff, &taste);
    let step = f
        .contradictions
        .iter()
        .find(|c| c.kind == ContradictionKind::Seniority)
        .expect("noted");
    assert_eq!(step.severity, Severity::Minor);
    assert!(step.text.contains("your latest title"), "{step:#?}");
    assert_eq!(f.level, FitLevel::Strong, "{f:#?}");

    let taste = profile(&[
        said(Seniority, "staff_plus", Polarity::Prefer),
        said(WorkShape, "backend", Polarity::Prefer),
        said(Team, "small_team", Polarity::Prefer),
    ]);
    let f = fit("Senior Backend Engineer", STARTUP_BACKEND, &staff, &taste);
    assert!(
        f.contradictions
            .iter()
            .any(|c| c.kind == ContradictionKind::Seniority && c.severity == Severity::Holding)
    );
    assert_ne!(f.level, FitLevel::Strong);
}

// ---------------------------------------------------------------------------
// Role depth.

const APP_POSTGRES: &str = "You'll own the schema design, query performance and partitioning of \
    our application data in PostgreSQL, and the services on top of it.";
const ENGINE_POSTGRES: &str = "Develop our PostgreSQL storage engine: the write-ahead log, MVCC and \
    the buffer manager.";

#[test]
fn using_postgresql_is_not_building_its_storage_engine() {
    let person = senior_engineer();
    let app = fit(
        "Senior Backend Engineer",
        APP_POSTGRES,
        &person,
        &generalist(),
    );
    assert!(
        !has_contradiction(&app, ContradictionKind::RoleDepth),
        "{app:#?}"
    );
    assert!(
        app.reasons
            .iter()
            .any(|r| r.text.contains("not database-engine internals"))
    );
    let engine = fit(
        "Senior Software Engineer",
        ENGINE_POSTGRES,
        &person,
        &generalist(),
    );
    assert_eq!(engine.level, FitLevel::Poor);
    assert!(has_contradiction(&engine, ContradictionKind::RoleDepth));
}

#[test]
fn the_storage_engine_is_a_strong_fit_for_someone_who_builds_them() {
    let specialist_taste = profile(&[
        said(WorkShape, "database_internals", Polarity::Prefer),
        said(Specialization, "deep", Polarity::Prefer),
        said(WorkShape, "backend", Polarity::Avoid),
    ]);
    let mut specialist = senior_engineer();
    specialist.specialties = crate::facets::work::specialties_in(
        "",
        &[(
            "Designed undo-log MVCC and the write-ahead log of our storage engine.".to_owned(),
            jobhunt_profile::words::words(
                "Designed undo-log MVCC and the write-ahead log of our storage engine.",
            ),
        )],
    );
    let engine = fit(
        "OrioleDB Developer",
        ENGINE_POSTGRES,
        &specialist,
        &specialist_taste,
    );
    assert_eq!(engine.level, FitLevel::Strong, "{engine:#?}");
    let app = fit(
        "Senior Backend Engineer",
        APP_POSTGRES,
        &specialist,
        &specialist_taste,
    );
    assert_eq!(app.level, FitLevel::Poor, "application work they avoid");
}

#[test]
fn running_on_kubernetes_is_not_writing_its_control_plane() {
    let taste = profile(&[
        said(WorkShape, "platform", Polarity::Prefer),
        said(Specialization, "deep", Polarity::Avoid),
    ]);
    let person = senior_engineer();
    let services = fit(
        "Senior Platform Engineer",
        "You'll run our services on Kubernetes and own the deploy pipeline and Helm charts.",
        &person,
        &taste,
    );
    assert!(!has_contradiction(&services, ContradictionKind::RoleDepth));
    let control = fit(
        "Senior Software Engineer, Kubernetes Control Plane",
        "Custom controllers and operators, the API machinery and etcd performance.",
        &person,
        &taste,
    );
    assert_eq!(control.level, FitLevel::Poor);
    assert!(has_contradiction(&control, ContradictionKind::RoleDepth));
}

#[test]
fn building_with_llm_apis_is_not_training_models() {
    let taste = profile(&[said(WorkShape, "product", Polarity::Prefer)]);
    let person = senior_engineer();
    let product = fit(
        "Senior Product Engineer, AI",
        "Integrate LLM APIs into the product. No machine learning background needed.",
        &person,
        &taste,
    );
    assert!(product.contradictions.is_empty(), "{product:#?}");
    let research = fit(
        "Research Engineer, Pretraining",
        "Scale distributed training and write CUDA kernels for frontier models.",
        &person,
        &taste,
    );
    assert_eq!(research.level, FitLevel::Poor);
    assert!(has_contradiction(&research, ContradictionKind::RoleDepth));
}

// ---------------------------------------------------------------------------
// Company shape.

#[test]
fn a_team_name_stands_in_only_for_work_done_on_that_teams_systems() {
    let taste = profile(&[said(WorkShape, "platform", Polarity::Prefer)]);
    let ios = fit(
        "iOS Engineer - User Platform",
        "",
        &senior_engineer(),
        &taste,
    );
    assert!(
        ios.contradictions
            .iter()
            .any(|c| c.kind == ContradictionKind::WorkShape && c.text.contains("names the team")),
        "{ios:#?}"
    );
    assert_ne!(ios.level, FitLevel::Strong);

    let taste = profile(&[said(WorkShape, "infrastructure", Polarity::Prefer)]);
    let security = fit("Security Engineer, Cloud", "", &senior_engineer(), &taste);
    assert!(security.contradictions.is_empty(), "{security:#?}");
    assert!(
        security
            .reasons
            .iter()
            .any(|r| r.aspect == Aspect::Role && r.text.contains("close to the infrastructure")),
        "{security:#?}"
    );
}

#[test]
fn wanting_startups_is_a_contradiction_for_a_public_giant() {
    let taste = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Company, "startup", Polarity::Prefer),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        "We are a publicly traded company with thousands of employees. You'll own backend services.",
        &senior_engineer(),
        &taste,
    );
    assert_eq!(f.level, FitLevel::Poor);
    assert!(has_contradiction(&f, ContradictionKind::CompanyShape));
}

#[test]
fn a_small_team_inside_a_large_company_is_the_teams_size() {
    let taste = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Team, "small_team", Polarity::Prefer),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        "We are a company of 4,000 people. You'll join a team of 5 engineers that owns billing.",
        &senior_engineer(),
        &taste,
    );
    assert!(f.reasons.iter().any(|r| r.aspect == Aspect::Team), "{f:#?}");
    assert!(
        !f.contradictions
            .iter()
            .any(|c| c.kind == ContradictionKind::TeamShape)
    );
}

#[test]
fn unknown_company_facts_are_neither_for_nor_against() {
    let f = fit(
        "Senior Backend Engineer",
        "You'll build backend services in Go.",
        &senior_engineer(),
        &generalist(),
    );
    assert!(f.contradictions.is_empty(), "{f:#?}");
    // The role stands on its own (the work they want, described, at their
    // level); the company side is unknown, said, and not held against it.
    assert_eq!(f.role, RoleFit::Strong, "{f:#?}");
    assert_eq!(f.company, CompanyFit::Unknown);
    assert_eq!(f.level, FitLevel::Strong);
    assert!(!f.uncertainties.is_empty());
}

// ---------------------------------------------------------------------------
// Role first, then the company (fit-rules/3).

/// Everything the company side can say, as well as it can say it.
const DREAM_COMPANY: &str = "We are a 12-person startup and a small team with high ownership:     real autonomy, no process for its own sake.";

#[test]
fn company_evidence_never_makes_a_strong_fit() {
    let generalist = generalist();
    // Work only related to what they want (full-stack, for a backend
    // engineer), at the best company they could describe.
    let related = fit(
        "Senior Full-Stack Engineer",
        &format!("{DREAM_COMPANY} You'll build product features across the web app."),
        &senior_engineer(),
        &generalist,
    );
    assert_eq!(related.company, CompanyFit::Matches, "{related:#?}");
    assert!(related.company_support >= 2.0, "{related:#?}");
    assert_eq!(related.role, RoleFit::Plausible, "{related:#?}");
    assert_eq!(related.level, FitLevel::Plausible);
    // The work they want, but only as words in the requirements.
    let listed = fit(
        "Software Engineer",
        &format!("{DREAM_COMPANY}\nRequirements\n- Go, PostgreSQL, gRPC, REST APIs, Redis"),
        &senior_engineer(),
        &generalist,
    );
    assert_ne!(listed.level, FitLevel::Strong, "{listed:#?}");
    // A level below the one they want isn't made up for by the company.
    let below = fit(
        "Backend Engineer II",
        &format!("{DREAM_COMPANY} You'll own backend services end to end."),
        &senior_engineer(),
        &generalist,
    );
    assert!(
        below
            .contradictions
            .iter()
            .any(|c| c.kind == ContradictionKind::Seniority && c.severity == Severity::Holding),
        "{below:#?}"
    );
    assert_eq!(below.level, FitLevel::Plausible);
    // The same company with the work described: strong, and the company
    // orders it above the same role elsewhere.
    let described = fit(
        "Senior Backend Engineer",
        &format!("{DREAM_COMPANY} You'll own backend services end to end in Go."),
        &senior_engineer(),
        &generalist,
    );
    let elsewhere = fit(
        "Senior Backend Engineer",
        "You'll own backend services end to end in Go.",
        &senior_engineer(),
        &generalist,
    );
    assert_eq!(described.level, FitLevel::Strong, "{described:#?}");
    assert_eq!(elsewhere.level, FitLevel::Strong, "{elsewhere:#?}");
    assert!(described.score() > elsewhere.score());
}

#[test]
fn role_and_company_reasons_stay_apart() {
    let f = fit(
        "Senior Backend Engineer",
        &format!("{DREAM_COMPANY} You'll own backend services end to end in Go."),
        &senior_engineer(),
        &generalist(),
    );
    assert!(f.role_reasons().all(|r| r.aspect.scope() == Scope::Role));
    assert!(f.role_reasons().any(|r| r.aspect == Aspect::Role));
    assert!(f.company_reasons().any(|r| r.aspect == Aspect::Team));
    assert!(
        f.company_reasons()
            .all(|r| r.aspect.scope() == Scope::Company)
    );
}

#[test]
fn a_title_alone_needs_one_more_job_local_fact() {
    // "Backend Engineer" and nothing about the work or the level.
    let bare = fit(
        "Backend Engineer",
        DREAM_COMPANY,
        &senior_engineer(),
        &generalist(),
    );
    assert_eq!(bare.role_basis, RoleBasis::Title, "{bare:#?}");
    assert_eq!(bare.level, FitLevel::Plausible);
    // The level they want is that fact.
    let senior = fit(
        "Senior Backend Engineer",
        DREAM_COMPANY,
        &senior_engineer(),
        &generalist(),
    );
    assert_eq!(senior.level, FitLevel::Strong, "{senior:#?}");
}

#[test]
fn a_company_headcount_from_another_posting_holds() {
    let taste = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Seniority, "senior", Polarity::Prefer),
        said(Company, "large_company", Polarity::Avoid),
    ]);
    let stated = facets(&record(
        "greenhouse:bigco",
        "Senior Backend Engineer",
        "The company is a pioneer of distributed work, with 1200+ colleagues in 75+ countries.",
    ));
    let silent = facets(&record(
        "greenhouse:bigco",
        "Senior Backend Engineer, Billing",
        "You'll own backend services end to end.",
    ));
    let book = CompanyBook::of([&stated, &silent]);
    let alone = assess(&silent, &senior_engineer(), &taste);
    assert_eq!(alone.level, FitLevel::Strong, "{alone:#?}");
    let with = assess_with(
        &silent,
        &senior_engineer(),
        &taste,
        book.get(&silent.company),
    );
    assert_eq!(with.company, CompanyFit::Conflicts, "{with:#?}");
    assert_eq!(with.level, FitLevel::Poor);
}

// ---------------------------------------------------------------------------
// Whose statement it is.

#[test]
fn the_persons_statement_counts_more_than_a_reading() {
    let job = ("Senior Backend Engineer", STARTUP_BACKEND);
    let theirs = fit(
        job.0,
        job.1,
        &senior_engineer(),
        &profile(&[said(WorkShape, "backend", Polarity::Prefer)]),
    );
    let read = fit(
        job.0,
        job.1,
        &senior_engineer(),
        &profile(&[read_from_words(
            WorkShape,
            "backend",
            Polarity::Prefer,
            TasteConfidence::Medium,
        )]),
    );
    assert!(theirs.role_fit > read.role_fit);
    assert_eq!(theirs.reasons[0].firmness, Firmness::Firm);
    assert_eq!(read.reasons[0].firmness, Firmness::Soft);
}

#[test]
fn avoided_values_are_contradictions_and_neutral_ones_nothing() {
    let avoid = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Company, "startup", Polarity::Avoid),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &senior_engineer(),
        &avoid,
    );
    assert!(
        has_contradiction(&f, ContradictionKind::CompanyShape),
        "{f:#?}"
    );
    let neutral = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Company, "startup", Polarity::Neutral),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &senior_engineer(),
        &neutral,
    );
    assert!(f.contradictions.is_empty());
    assert!(!f.reasons.iter().any(|r| r.aspect == Aspect::Company));
}

#[test]
fn learned_only_patterns_are_weaker_and_never_override_the_person() {
    // Learned avoidance: said, and holds a job back only when established.
    let tentative = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Team, "small_team", Polarity::Prefer),
        learned(Company, "startup", Polarity::Avoid, TasteConfidence::Low),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &senior_engineer(),
        &tentative,
    );
    assert_eq!(f.level, FitLevel::Strong, "{f:#?}");
    assert!(
        f.contradictions
            .iter()
            .any(|c| c.severity == Severity::Minor)
    );
    let strong = profile(&[
        said(WorkShape, "backend", Polarity::Prefer),
        said(Team, "small_team", Polarity::Prefer),
        learned(Company, "startup", Polarity::Avoid, TasteConfidence::High),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &senior_engineer(),
        &strong,
    );
    assert_eq!(
        f.level,
        FitLevel::Plausible,
        "held back, not ruled out: {f:#?}"
    );
    // Learned preferences alone never make a strong fit.
    let only_learned = profile(&[
        learned(
            WorkShape,
            "backend",
            Polarity::Prefer,
            TasteConfidence::High,
        ),
        learned(Team, "small_team", Polarity::Prefer, TasteConfidence::High),
        learned(Ownership, "high", Polarity::Prefer, TasteConfidence::High),
    ]);
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &Person::default(),
        &only_learned,
    );
    assert_ne!(f.level, FitLevel::Strong, "{f:#?}");
}

#[test]
fn removed_statements_say_nothing() {
    use chrono::TimeZone;
    use jobhunt_profile::taste::{TasteAssertion, TasteSource, compose};
    use jobhunt_profile::{ProfileData, ProfileId};
    let at = chrono::Utc.with_ymd_and_hms(2026, 9, 1, 0, 0, 0).unwrap();
    let mut data = ProfileData::new(ProfileId::local(), at);
    let statement = |value: &str, polarity, review| TasteAssertion {
        id: TasteId::derive(&[value]),
        dimension: WorkShape,
        value: value.into(),
        polarity,
        text: value.into(),
        confidence: TasteConfidence::High,
        origin: TasteOrigin::Interpreted,
        review,
        sources: vec![TasteSource::Person],
        explanation: None,
        interpreter: None,
        original: None,
        superseded_by: None,
        created_at: at,
        updated_at: at,
    };
    data.taste
        .push(statement("backend", Polarity::Avoid, TasteReview::Removed));
    data.taste.push(statement(
        "platform",
        Polarity::Prefer,
        TasteReview::Confirmed,
    ));
    let taste = compose(&data, &[]);
    let f = fit(
        "Senior Backend Engineer",
        STARTUP_BACKEND,
        &senior_engineer(),
        &taste,
    );
    assert!(
        !f.contradictions
            .iter()
            .any(|c| c.kind == ContradictionKind::WorkShape),
        "a removed avoidance is a tombstone, not a contradiction: {f:#?}"
    );
}

// ---------------------------------------------------------------------------
// Pay is not fit.

#[test]
fn fit_never_reads_pay() {
    let rich = "You'll own backend services end to end. The salary is USD 400,000 - 500,000 \
        per year.";
    let poor =
        "You'll own backend services end to end. The salary is USD 40,000 - 50,000 per year.";
    let person = senior_engineer();
    let a = fit("Senior Backend Engineer", rich, &person, &generalist());
    let b = fit("Senior Backend Engineer", poor, &person, &generalist());
    assert_eq!(a.level, b.level);
    assert_eq!(a.score(), b.score());
    assert!(
        !a.reasons
            .iter()
            .any(|r| r.text.to_lowercase().contains("salary"))
    );
}
