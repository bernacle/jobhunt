//! Builders for unit tests.

#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{DateTime, Duration, TimeZone, Utc};
use jobhunt_core::{CanonicalUrl, Provenance, SourceKey};
use jobhunt_eligibility::{Assessment, ProfileFacts, assess};
use jobhunt_jobs::verification::{
    ApplicationCheck, Authority, CompensationCheck, FreshnessPolicy, ListingStatus, PublishedPlace,
    RecordVerification, VERIFICATION_REVISION, VerificationId, VerificationMethod,
    VerificationRecord,
};
use jobhunt_jobs::{
    Compensation, CompensationComponent, CompensationKind, JobPosting, JobRecord, JobStatus,
    OpportunityId, PayInterval, WorkplaceType,
};

use crate::feedback::{FeedbackAction, FeedbackEvent};

pub fn now() -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 9, 25, 12, 0, 0).unwrap()
}

/// A stored, open, remote job first seen three days ago.
pub fn record(source: &str, title: &str, description: &str) -> JobRecord {
    let key: SourceKey = source.parse().unwrap();
    let native = jobhunt_core::StableId::derive("test", &[source, title, description]).to_hex();
    let posting = JobPosting {
        provenance: Provenance {
            source: key.clone(),
            source_record_id: Some(native.clone()),
            fetched_from: None,
        },
        url: CanonicalUrl::parse(&format!(
            "https://jobs.example.com/{}/{native}",
            key.instance()
        ))
        .unwrap(),
        apply_url: None,
        company: "Acme".into(),
        title: title.into(),
        department: None,
        team: None,
        location: Some("Remote".into()),
        locations: Vec::new(),
        employment_type: None,
        workplace_type: Some(WorkplaceType::Remote),
        is_remote: Some(true),
        compensation: None,
        work_authorization: None,
        description_text: (!description.is_empty()).then(|| description.to_owned()),
        description_html: None,
        posted_at: None,
        source_updated_at: None,
    };
    let seen = now() - Duration::days(3);
    JobRecord {
        id: posting.id(),
        opportunity_id: OpportunityId::founded_by(posting.id()),
        posting,
        first_seen_at: seen,
        last_seen_at: seen,
        content_updated_at: seen,
        status: JobStatus::Open,
        closed_at: None,
    }
}

/// A yearly salary range.
pub fn salary(currency: Option<&str>, min: f64, max: f64, summary: Option<&str>) -> Compensation {
    Compensation {
        summary: summary.map(str::to_owned),
        components: vec![CompensationComponent {
            kind: CompensationKind::Salary,
            label: None,
            currency: currency.map(str::to_owned),
            min: Some(min),
            max: Some(max),
            interval: Some(PayInterval::Year),
        }],
    }
}

/// A successful verification of `record`, `hours` ago, finding it active.
pub fn verified(record: &JobRecord, hours: i64) -> VerificationRecord {
    let at = now() - Duration::hours(hours);
    VerificationRecord {
        id: VerificationId::derive(record.id, at),
        job_id: record.id,
        opportunity_id: record.opportunity_id,
        source: record.posting.provenance.source.clone(),
        source_record_id: record.posting.provenance.source_record_id.clone(),
        attempted_at: at,
        method: VerificationMethod::AshbyBoardApi,
        listing: ListingStatus::Active,
        application: ApplicationCheck::unknown("not checked in tests"),
        authority: Authority::EmployerConfiguredAts,
        authority_chain: Vec::new(),
        checked_url: None,
        listing_url: None,
        content_fingerprint: None,
        changed_fields: Vec::new(),
        changed_since_last_verification: None,
        lifecycle: None,
        compensation: CompensationCheck::observe(record.posting.compensation.as_ref(), None),
        published: PublishedPlace::default(),
        unknowns: Vec::new(),
        failure: None,
        revision: VERIFICATION_REVISION.to_owned(),
    }
}

/// The assessment of `record` for someone living in `place`, verified
/// `hours` ago (never, with `None`).
pub fn assessment(record: &JobRecord, place: &str, hours: Option<i64>) -> Assessment {
    assessment_for(record, &ProfileFacts::living_in(place), hours)
}

/// The assessment of `record` for a person's facts, verified `hours` ago
/// (never, with `None`).
pub fn assessment_for(record: &JobRecord, facts: &ProfileFacts, hours: Option<i64>) -> Assessment {
    let v = hours.map(|h| verified(record, h));
    let rv = RecordVerification {
        record: record.clone(),
        latest: v.clone(),
        last_success: v,
        reused: true,
    };
    assess(&[rv], facts, &FreshnessPolicy::default(), now())
}

/// An event on a generic record, `minutes` after [`now`].
pub fn event(action: FeedbackAction, reason: Option<&str>, minutes: i64) -> FeedbackEvent {
    let r = record("ashby:acme", "Backend Engineer", "");
    event_on(&r, action, reason, minutes)
}

/// An event on `record`, `minutes` after [`now`].
pub fn event_on(
    record: &JobRecord,
    action: FeedbackAction,
    reason: Option<&str>,
    minutes: i64,
) -> FeedbackEvent {
    FeedbackEvent::new(
        "prof_test",
        record.opportunity_id,
        record.id,
        action,
        reason,
        (&record.posting.title, &record.posting.company),
        now() + Duration::minutes(minutes),
    )
}

/// The taste profile `compose` would give a person whose only taste is
/// their structured preferences (read as earlier settings, entered by them).
pub fn taste_of(person: &crate::person::Person) -> jobhunt_profile::taste::TasteProfile {
    taste_with(person, &crate::taste::TasteModel::empty("rules/1"))
}

/// [`taste_of`], with the patterns learned in `learned` composed as
/// [`fn@jobhunt_profile::taste::compose`] composes them (a structured
/// preference about the same key wins).
pub fn taste_with(
    person: &crate::person::Person,
    learned: &crate::taste::TasteModel,
) -> jobhunt_profile::taste::TasteProfile {
    use jobhunt_profile::TasteId;
    use jobhunt_profile::taste::{ComposedAssertion, TasteOrigin, TasteReview, vocab};
    let mut out = stated_taste(person);
    for s in crate::taste::learned_signals(learned) {
        let key = jobhunt_profile::taste::key(s.dimension, &s.value);
        if out.assertions.iter().any(|a| a.key() == key) {
            continue;
        }
        out.assertions.push(ComposedAssertion {
            id: TasteId::derive(&["learned", &key]),
            dimension: s.dimension,
            text: vocab::sentence(s.dimension, &s.value, s.polarity),
            value: s.value,
            polarity: s.polarity,
            confidence: s.confidence,
            origin: TasteOrigin::Learned,
            review: TasteReview::Unreviewed,
            sources: Vec::new(),
            against: Vec::new(),
            explanation: None,
            interpreter: None,
            original: None,
            stored: false,
        });
    }
    out
}

fn stated_taste(person: &crate::person::Person) -> jobhunt_profile::taste::TasteProfile {
    use jobhunt_profile::taste::{
        ComposedAssertion, TasteConfidence, TasteOrigin, TasteProfile, TasteReview, vocab,
    };
    use jobhunt_profile::{CompanyTrait, TasteDimension, TasteId, WorkAspect};

    use crate::key::Dimension;
    use crate::person::Matcher;

    let mut out = TasteProfile::default();
    for p in &person.stated {
        let targets: Vec<(TasteDimension, String)> = match (&p.matcher, p.dimension) {
            (Matcher::Keys(keys), Dimension::Role) => keys
                .iter()
                .filter_map(|k| match k.dimension {
                    Dimension::Role => Some((
                        TasteDimension::WorkShape,
                        match k.value.as_str() {
                            "full stack" => "full_stack".to_owned(),
                            "machine learning" => "ml_product".to_owned(),
                            other => other.to_owned(),
                        },
                    )),
                    Dimension::Seniority => vocab::from_learned("seniority", &k.value),
                    _ => None,
                })
                .collect(),
            (Matcher::TitleWords(words), Dimension::Role) => vocab::role(words),
            (Matcher::Keys(keys), Dimension::CompanyTrait) => keys
                .iter()
                .filter_map(|k| CompanyTrait::from_canonical(&k.value))
                .map(|t| {
                    let (d, v) = vocab::company_trait(t);
                    (d, v.to_owned())
                })
                .collect(),
            (Matcher::Keys(keys), Dimension::WorkStyle) => keys
                .iter()
                .filter_map(|k| WorkAspect::from_canonical(&k.value))
                .map(|a| {
                    let (d, v) = vocab::work_aspect(a);
                    (d, v.to_owned())
                })
                .collect(),
            (Matcher::Keys(keys), Dimension::Domain) => keys
                .iter()
                .map(|k| (TasteDimension::Domain, k.value.clone()))
                .collect(),
            _ => Vec::new(),
        };
        for (dimension, value) in targets {
            let polarity = vocab::polarity_of(p.stance);
            out.assertions.push(ComposedAssertion {
                id: TasteId::derive(&[&p.id, &value]),
                dimension,
                text: vocab::sentence(dimension, &value, polarity),
                value,
                polarity,
                confidence: if p.uncertain {
                    TasteConfidence::Medium
                } else {
                    TasteConfidence::High
                },
                origin: TasteOrigin::Legacy,
                review: TasteReview::Unreviewed,
                sources: Vec::new(),
                against: Vec::new(),
                explanation: None,
                interpreter: None,
                original: None,
                stored: false,
            });
        }
    }
    out
}
