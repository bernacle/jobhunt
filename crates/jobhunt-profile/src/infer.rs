//! Vocabularies and the rules that turn resume text into technology,
//! domain, role and ownership evidence.
//!
//! Everything here is deterministic and explainable: a match always
//! returns the words that triggered it, which become the claim's snippet or
//! [`crate::Claim::basis`]. Technology mentions are *extracted* (the
//! resume names the technology); domains, role kinds and ownership are
//! *inferred* and always need the user's confirmation before use.

use std::sync::LazyLock;

use crate::words::{Pattern, Vocabulary, Word, span_text, words};

/// A known technology.
#[derive(Debug)]
pub struct Technology {
    /// Canonical display name.
    pub name: &'static str,
    pub category: &'static str,
    patterns: &'static [&'static str],
    /// Too ambiguous to recognize in prose ("C", "R"); only in lists.
    list_only: bool,
}

impl Technology {
    /// The topic key used on claims and skills.
    pub fn key(&self) -> String {
        topic_key(self.name)
    }
}

macro_rules! tech {
    ($name:literal, $cat:literal, [$($p:literal),+ $(,)?]) => {
        Technology { name: $name, category: $cat, patterns: &[$($p),+], list_only: false }
    };
    ($name:literal, $cat:literal, [$($p:literal),+ $(,)?], list_only) => {
        Technology { name: $name, category: $cat, patterns: &[$($p),+], list_only: true }
    };
}

/// Longer names come first so that "React Native" wins over "React".
static TECHNOLOGIES: &[Technology] = &[
    tech!("React Native", "mobile", ["react native"]),
    tech!(
        "Ruby on Rails",
        "backend framework",
        ["ruby on rails", "=Rails"]
    ),
    tech!("Spring Boot", "backend framework", ["spring boot"]),
    tech!("GitHub Actions", "tooling", ["github actions"]),
    tech!("Google Cloud", "cloud", ["google cloud", "gcp"]),
    tech!("Next.js", "frontend", ["next.js", "nextjs"]),
    tech!(
        "Node.js",
        "backend framework",
        ["node.js", "nodejs", "=Node"]
    ),
    tech!("TypeScript", "language", ["typescript"]),
    tech!("JavaScript", "language", ["javascript"]),
    tech!("Rust", "language", ["rust"]),
    tech!("Go", "language", ["golang", "=Go"]),
    tech!("Python", "language", ["python"]),
    tech!("Java", "language", ["java"]),
    tech!("Kotlin", "language", ["kotlin"]),
    tech!("Scala", "language", ["scala"]),
    tech!("Ruby", "language", ["=Ruby"]),
    tech!("PHP", "language", ["php"]),
    tech!("C#", "language", ["c#"]),
    tech!("C++", "language", ["c++"]),
    tech!("C", "language", ["=C"], list_only),
    tech!("R", "language", ["=R"], list_only),
    tech!("Elixir", "language", ["elixir"]),
    tech!("Erlang", "language", ["erlang"]),
    tech!("Haskell", "language", ["haskell"]),
    tech!("Clojure", "language", ["clojure"]),
    tech!("OCaml", "language", ["ocaml"]),
    tech!("Swift", "language", ["=Swift"]),
    tech!("Objective-C", "language", ["objective c"]),
    tech!("Dart", "language", ["=Dart"]),
    tech!("Flutter", "mobile", ["flutter"]),
    tech!("SQL", "language", ["sql"]),
    tech!("PostgreSQL", "database", ["postgresql", "postgres", "psql"]),
    tech!("MySQL", "database", ["mysql"]),
    tech!("SQLite", "database", ["sqlite"]),
    tech!("MongoDB", "database", ["mongodb", "mongo"]),
    tech!("Redis", "database", ["redis"]),
    tech!("Cassandra", "database", ["cassandra"]),
    tech!("DynamoDB", "database", ["dynamodb"]),
    tech!(
        "Elasticsearch",
        "database",
        ["elasticsearch", "elastic search", "opensearch"]
    ),
    tech!("ClickHouse", "database", ["clickhouse"]),
    tech!("Kafka", "messaging", ["kafka"]),
    tech!("RabbitMQ", "messaging", ["rabbitmq"]),
    tech!("NATS", "messaging", ["=NATS"]),
    tech!("SQS", "messaging", ["sqs"]),
    tech!("gRPC", "backend framework", ["grpc"]),
    tech!("GraphQL", "backend framework", ["graphql"]),
    tech!("Kubernetes", "infrastructure", ["kubernetes", "k8s"]),
    tech!("Docker", "infrastructure", ["docker"]),
    tech!("Terraform", "infrastructure", ["terraform"]),
    tech!("Ansible", "infrastructure", ["ansible"]),
    tech!("Helm", "infrastructure", ["=Helm"]),
    tech!("AWS", "cloud", ["aws", "amazon web services"]),
    tech!("Azure", "cloud", ["azure"]),
    tech!("Cloudflare", "cloud", ["cloudflare"]),
    tech!("Vercel", "cloud", ["vercel"]),
    tech!("Linux", "infrastructure", ["linux"]),
    tech!("Nginx", "infrastructure", ["nginx"]),
    tech!("Prometheus", "observability", ["prometheus"]),
    tech!("Grafana", "observability", ["grafana"]),
    tech!("Datadog", "observability", ["datadog"]),
    tech!("OpenTelemetry", "observability", ["opentelemetry"]),
    tech!("React", "frontend", ["react", "react.js", "reactjs"]),
    tech!("Vue", "frontend", ["=Vue", "vue.js", "vuejs"]),
    tech!("Angular", "frontend", ["=Angular", "angularjs"]),
    tech!("Svelte", "frontend", ["svelte", "sveltekit"]),
    tech!("Tailwind CSS", "frontend", ["tailwind"]),
    tech!("HTML", "frontend", ["html", "html5"]),
    tech!("CSS", "frontend", ["css", "css3"]),
    tech!(
        "Express",
        "backend framework",
        ["=Express", "express.js", "expressjs"]
    ),
    tech!("NestJS", "backend framework", ["nestjs"]),
    tech!("Django", "backend framework", ["django"]),
    tech!("Flask", "backend framework", ["=Flask"]),
    tech!("FastAPI", "backend framework", ["fastapi"]),
    tech!("Spring", "backend framework", ["=Spring"]),
    tech!(".NET", "backend framework", [".net", "dotnet"]),
    tech!("Tokio", "backend framework", ["tokio"]),
    tech!("Supabase", "backend framework", ["supabase"]),
    tech!("Firebase", "backend framework", ["firebase"]),
    tech!("Snowflake", "data", ["=Snowflake"]),
    tech!("BigQuery", "data", ["bigquery"]),
    tech!("Spark", "data", ["=Spark", "pyspark"]),
    tech!("Airflow", "data", ["=Airflow"]),
    tech!("dbt", "data", ["=dbt"]),
    tech!("Pandas", "data", ["pandas"]),
    tech!("PyTorch", "machine learning", ["pytorch"]),
    tech!("TensorFlow", "machine learning", ["tensorflow"]),
    tech!("WebAssembly", "tooling", ["webassembly", "wasm"]),
    tech!("Git", "tooling", ["=Git"]),
    tech!("Jenkins", "tooling", ["jenkins"]),
    tech!("CircleCI", "tooling", ["circleci"]),
];

static TECH_PATTERNS: LazyLock<Vec<(&'static Technology, Vec<Pattern>)>> = LazyLock::new(|| {
    TECHNOLOGIES
        .iter()
        .map(|t| (t, t.patterns.iter().map(|p| Pattern::new(p)).collect()))
        .collect()
});

/// Every technology pattern in one [`Vocabulary`], in [`TECH_PATTERNS`]
/// order, with the technology each belongs to.
static TECH_VOCABULARY: LazyLock<(Vocabulary, Vec<&'static Technology>)> = LazyLock::new(|| {
    let mut owners = Vec::new();
    let mut patterns = Vec::new();
    for (tech, compiled) in TECH_PATTERNS.iter() {
        for pattern in compiled {
            owners.push(*tech);
            patterns.push(pattern.clone());
        }
    }
    (Vocabulary::new(patterns), owners)
});

/// Normalized key for a topic: lowercase, single spaces.
pub fn topic_key(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// A technology mention in some text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mention {
    pub technology: &'static str,
    pub category: &'static str,
    /// The words as written.
    pub matched: String,
}

/// Every known technology named in `text`, once each, in order of first
/// mention. `list` enables names too ambiguous for prose ("C").
pub fn technologies_in(text: &str, list: bool) -> Vec<Mention> {
    technologies_in_words(text, &words(text), list)
}

/// [`technologies_in`] for text already split into `ws` (`words(text)`).
pub fn technologies_in_words(text: &str, ws: &[Word], list: bool) -> Vec<Mention> {
    let (vocabulary, owners) = &*TECH_VOCABULARY;
    let mut taken = vec![false; ws.len()];
    let mut found: Vec<(usize, Mention)> = Vec::new();
    // Pattern order is technology order, so earlier technologies claim
    // their words first, as they always have.
    for (id, ranges) in vocabulary.find_all(ws) {
        let tech = owners[id];
        if tech.list_only && !list {
            continue;
        }
        for range in ranges {
            if taken[range.clone()].iter().any(|t| *t) {
                continue;
            }
            taken[range.clone()].iter_mut().for_each(|t| *t = true);
            if !found.iter().any(|(_, m)| m.technology == tech.name) {
                found.push((
                    range.start,
                    Mention {
                        technology: tech.name,
                        category: tech.category,
                        matched: span_text(text, ws, &range).to_owned(),
                    },
                ));
            }
        }
    }
    found.sort_by_key(|(at, _)| *at);
    found.into_iter().map(|(_, m)| m).collect()
}

/// The canonical technology a skill name refers to, if it is a known one
/// ("postgres" → PostgreSQL).
pub fn known_technology(name: &str) -> Option<&'static Technology> {
    let ws = words(name);
    TECH_PATTERNS
        .iter()
        .find(|(_, patterns)| {
            patterns
                .iter()
                .any(|p| p.len() == ws.len() && p.find(&ws) == Some(0..ws.len()))
        })
        .map(|(t, _)| *t)
        .or_else(|| {
            let key = topic_key(name);
            TECHNOLOGIES.iter().find(|t| topic_key(t.name) == key)
        })
}

struct DomainTerms {
    domain: &'static str,
    /// Phrases naming the domain outright ("payments", "compliance").
    strong: &'static [&'static str],
    /// Phrases typical of the domain.
    weak: &'static [&'static str],
}

static DOMAINS: &[DomainTerms] = &[
    DomainTerms {
        domain: "payments",
        strong: &["payment*", "psp", "acquirer*", "acquiring"],
        weak: &[
            "card authori*",
            "card issuing",
            "checkout",
            "settlement*",
            "=PIX",
            "remittance*",
            "payout*",
            "merchant*",
            "chargeback*",
        ],
    },
    DomainTerms {
        domain: "banking",
        strong: &["bank", "banks", "banking", "=Banco", "neobank*"],
        weak: &["ledger*", "account onboarding", "core banking"],
    },
    DomainTerms {
        domain: "fintech",
        strong: &["fintech*"],
        weak: &[
            "lending",
            "credit",
            "brokerage",
            "trading",
            "wealth",
            "personal finance",
            "budgeting",
            "financial service*",
        ],
    },
    DomainTerms {
        domain: "compliance",
        strong: &[
            "compliance",
            "regulatory",
            "=KYC",
            "=AML",
            "anti money laundering",
        ],
        weak: &["travel rule", "sanction*", "audit*", "=KYB"],
    },
    DomainTerms {
        domain: "crypto",
        strong: &[
            "crypto*",
            "blockchain*",
            "web3",
            "stablecoin*",
            "virtual asset*",
        ],
        weak: &["bitcoin", "ethereum", "defi", "=VASP*", "wallet*"],
    },
    DomainTerms {
        domain: "fraud and risk",
        strong: &["fraud"],
        weak: &["risk scoring", "risk team", "risk engine", "risk model*"],
    },
    DomainTerms {
        domain: "security",
        strong: &["security", "appsec", "cybersecurity"],
        weak: &[
            "encryption",
            "vulnerabilit*",
            "threat*",
            "zero trust",
            "penetration test*",
        ],
    },
    DomainTerms {
        domain: "infrastructure",
        strong: &["infrastructure", "infra"],
        weak: &[
            "provision*",
            "cluster*",
            "on call",
            "availability",
            "staging environment*",
            "load balanc*",
            "networking",
        ],
    },
    DomainTerms {
        domain: "developer tools",
        strong: &[
            "developer tool*",
            "devtool*",
            "dev tool*",
            "developer experience",
            "developer platform*",
        ],
        weak: &["sdk*", "=CLI", "librar*", "=IDE*", "compiler*"],
    },
    DomainTerms {
        domain: "ai",
        strong: &[
            "=AI",
            "machine learning",
            "=ML",
            "=LLM*",
            "large language model*",
            "artificial intelligence",
            "deep learning",
        ],
        weak: &[
            "=NLP",
            "computer vision",
            "recommendation*",
            "model training",
        ],
    },
    DomainTerms {
        domain: "data",
        strong: &["data platform*", "data engineering", "data warehouse*"],
        weak: &["etl", "data pipeline*", "analytics"],
    },
    DomainTerms {
        domain: "b2b saas",
        strong: &["saas"],
        weak: &["multi tenant", "enterprise customer*", "subsidiar*"],
    },
    DomainTerms {
        domain: "e-commerce",
        strong: &["e commerce", "ecommerce"],
        weak: &["online store*", "marketplace*", "retail", "shopping"],
    },
    DomainTerms {
        domain: "healthcare",
        strong: &["healthcare", "health tech", "healthtech", "medical"],
        weak: &["patient*", "clinical", "hospital*"],
    },
    DomainTerms {
        domain: "education",
        strong: &["edtech", "education"],
        weak: &["student*", "learning platform*"],
    },
    DomainTerms {
        domain: "gaming",
        strong: &["gaming", "video game*", "game studio*"],
        weak: &[],
    },
    DomainTerms {
        domain: "gambling",
        strong: &["gambling", "betting", "casino*", "sportsbook*", "igaming"],
        weak: &[],
    },
    DomainTerms {
        domain: "adtech",
        strong: &["adtech", "ad tech", "advertising"],
        weak: &["ad serving", "programmatic"],
    },
    DomainTerms {
        domain: "logistics",
        strong: &["logistics", "supply chain"],
        weak: &["shipping", "fleet*", "last mile"],
    },
    DomainTerms {
        domain: "insurance",
        strong: &["insurance", "insurtech"],
        weak: &[],
    },
    DomainTerms {
        domain: "climate",
        strong: &["climate", "clean energy", "renewable*"],
        weak: &["carbon", "emission*"],
    },
    DomainTerms {
        domain: "hr tech",
        strong: &["hr tech", "hrtech", "recruiting software"],
        weak: &["payroll", "recruiting", "hiring platform*"],
    },
    DomainTerms {
        domain: "government",
        strong: &["government", "public sector", "govtech"],
        weak: &[],
    },
    DomainTerms {
        domain: "defense",
        strong: &["defense", "defence", "military"],
        weak: &[],
    },
    DomainTerms {
        domain: "social media",
        strong: &["social media", "social network*"],
        weak: &[],
    },
];

type CompiledDomain = (&'static str, Vec<Pattern>, Vec<Pattern>);

static DOMAIN_PATTERNS: LazyLock<Vec<CompiledDomain>> = LazyLock::new(|| {
    DOMAINS
        .iter()
        .map(|d| {
            (
                d.domain,
                d.strong.iter().map(|p| Pattern::new(p)).collect(),
                d.weak.iter().map(|p| Pattern::new(p)).collect(),
            )
        })
        .collect()
});

/// Every domain pattern in one [`Vocabulary`], in [`DOMAIN_PATTERNS`] order
/// (each domain's strong phrases, then its weak ones), with the domain and
/// whether the phrase is strong.
static DOMAIN_VOCABULARY: LazyLock<(Vocabulary, Vec<(&'static str, bool)>)> = LazyLock::new(|| {
    let mut owners = Vec::new();
    let mut patterns = Vec::new();
    for (domain, strong, weak) in DOMAIN_PATTERNS.iter() {
        for (compiled, is_strong) in [(strong, true), (weak, false)] {
            for pattern in compiled {
                owners.push((*domain, is_strong));
                patterns.push(pattern.clone());
            }
        }
    }
    (Vocabulary::new(patterns), owners)
});

/// A domain phrase found in text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainHit {
    pub domain: &'static str,
    pub phrase: String,
    /// The phrase names the domain outright.
    pub strong: bool,
}

/// Every domain phrase in `text`.
pub fn domains_in(text: &str) -> Vec<DomainHit> {
    domains_in_words(text, &words(text))
}

/// [`domains_in`] for text already split into `ws` (`words(text)`).
pub fn domains_in_words(text: &str, ws: &[Word]) -> Vec<DomainHit> {
    let (vocabulary, owners) = &*DOMAIN_VOCABULARY;
    let mut hits = Vec::new();
    for (id, ranges) in vocabulary.find_all(ws) {
        let (domain, strong) = owners[id];
        for range in ranges {
            hits.push(DomainHit {
                domain,
                phrase: span_text(text, ws, &range).to_owned(),
                strong,
            });
        }
    }
    hits
}

/// The canonical domain a phrase names, if any ("devtools" → "developer
/// tools"). Only strong phrases count.
pub fn canonical_domain(phrase: &str) -> Option<&'static str> {
    domains_in(phrase)
        .into_iter()
        .find(|h| h.strong)
        .map(|h| h.domain)
}

/// Every domain JobHunt knows by name.
pub fn known_domains() -> impl Iterator<Item = &'static str> {
    DOMAINS.iter().map(|d| d.domain)
}

/// Kinds of engineering roles evidence can point to.
pub const ROLE_KINDS: [&str; 10] = [
    "backend",
    "frontend",
    "full stack",
    "platform",
    "infrastructure",
    "data",
    "mobile",
    "machine learning",
    "security",
    "embedded",
];

struct RoleTerms {
    role: &'static str,
    title: &'static [&'static str],
    technologies: &'static [&'static str],
    phrases: &'static [&'static str],
}

static ROLES: &[RoleTerms] = &[
    RoleTerms {
        role: "backend",
        title: &["backend", "back end", "server*", "api"],
        technologies: &[
            "PostgreSQL",
            "MySQL",
            "Redis",
            "Kafka",
            "RabbitMQ",
            "gRPC",
            "GraphQL",
            "Java",
            "Spring Boot",
            "Spring",
            "Django",
            "Ruby on Rails",
            "Node.js",
            "Go",
            "Rust",
            "Elixir",
            "FastAPI",
            "Express",
            "NestJS",
            "DynamoDB",
            "MongoDB",
        ],
        phrases: &[
            "api*",
            "backend",
            "back end",
            "microservice*",
            "service*",
            "database*",
            "event pipeline*",
        ],
    },
    RoleTerms {
        role: "frontend",
        title: &["frontend", "front end", "ui", "web"],
        technologies: &[
            "React",
            "Vue",
            "Angular",
            "Svelte",
            "Next.js",
            "CSS",
            "HTML",
            "Tailwind CSS",
        ],
        phrases: &[
            "frontend",
            "front end",
            "=UI",
            "user interface*",
            "design system*",
            "web app*",
        ],
    },
    RoleTerms {
        role: "platform",
        title: &["platform"],
        technologies: &[],
        phrases: &[
            "platform team",
            "internal platform*",
            "developer platform*",
            "internal tool*",
            "ci cd",
        ],
    },
    RoleTerms {
        role: "infrastructure",
        title: &[
            "infrastructure",
            "infra",
            "sre",
            "site reliability",
            "devops",
            "cloud",
        ],
        technologies: &[
            "Kubernetes",
            "Terraform",
            "Ansible",
            "Helm",
            "Docker",
            "AWS",
            "Google Cloud",
            "Azure",
        ],
        phrases: &[
            "infrastructure",
            "provision*",
            "on call",
            "availability",
            "=SLO*",
            "incident*",
        ],
    },
    RoleTerms {
        role: "data",
        title: &["data"],
        technologies: &[
            "Spark",
            "Airflow",
            "dbt",
            "Snowflake",
            "BigQuery",
            "Pandas",
            "ClickHouse",
        ],
        phrases: &["data pipeline*", "etl", "data warehouse*", "analytics"],
    },
    RoleTerms {
        role: "mobile",
        title: &["mobile", "ios", "android"],
        technologies: &[
            "React Native",
            "Swift",
            "Kotlin",
            "Flutter",
            "Objective-C",
            "Dart",
        ],
        phrases: &["mobile app*", "=iOS", "android"],
    },
    RoleTerms {
        role: "machine learning",
        title: &["machine learning", "ml", "ai", "data scientist"],
        technologies: &["PyTorch", "TensorFlow"],
        phrases: &["machine learning", "model training", "=LLM*"],
    },
    RoleTerms {
        role: "security",
        title: &["security", "appsec"],
        technologies: &[],
        phrases: &["security", "vulnerabilit*", "threat model*"],
    },
    RoleTerms {
        role: "embedded",
        title: &["embedded", "firmware"],
        technologies: &[],
        phrases: &["firmware", "embedded", "microcontroller*"],
    },
];

/// What points to a kind of role in one experience or project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleSignal {
    pub role: &'static str,
    /// The title names the role kind.
    pub from_title: bool,
    /// Human-readable evidence ("title “Backend Engineer”", "uses PostgreSQL").
    pub reasons: Vec<String>,
}

/// Role-kind signals for one position: its title, the technologies it
/// used, and its bullet text. Full stack is signalled by the title or by
/// solid backend *and* frontend evidence together.
pub fn role_signals(title: Option<&str>, technologies: &[&str], texts: &[&str]) -> Vec<RoleSignal> {
    let text_words: Vec<Vec<Word>> = texts.iter().map(|t| words(t)).collect();
    let split: Vec<(&str, &[Word])> = texts
        .iter()
        .zip(&text_words)
        .map(|(t, ws)| (*t, ws.as_slice()))
        .collect();
    role_signals_in(title, technologies, &split)
}

/// [`ROLES`] with their patterns compiled once: (title, phrases).
static ROLE_PATTERNS: LazyLock<Vec<(Vocabulary, Vocabulary)>> = LazyLock::new(|| {
    ROLES
        .iter()
        .map(|r| (Vocabulary::compile(r.title), Vocabulary::compile(r.phrases)))
        .collect()
});

/// [`role_signals`] for texts already split into words: `(text, words(text))`.
pub fn role_signals_in(
    title: Option<&str>,
    technologies: &[&str],
    texts: &[(&str, &[Word])],
) -> Vec<RoleSignal> {
    let title_words = title.map(words).unwrap_or_default();
    let mut signals: Vec<RoleSignal> = Vec::new();
    for (terms, (title_patterns, phrase_patterns)) in ROLES.iter().zip(ROLE_PATTERNS.iter()) {
        let mut reasons = Vec::new();
        let from_title = title_patterns.any(&title_words);
        if from_title && let Some(title) = title {
            reasons.push(format!("title “{title}”"));
        }
        let techs: Vec<&str> = technologies
            .iter()
            .copied()
            .filter(|t| terms.technologies.contains(t))
            .collect();
        if !techs.is_empty() {
            reasons.push(format!("uses {}", techs.join(", ")));
        }
        let mut phrases: Vec<String> = Vec::new();
        for (text, ws) in texts {
            for (_, range) in phrase_patterns.find_first(ws) {
                let phrase = span_text(text, ws, &range).to_owned();
                if !phrases.iter().any(|x| x.eq_ignore_ascii_case(&phrase)) {
                    phrases.push(phrase);
                }
            }
        }
        if !phrases.is_empty() {
            let quoted: Vec<String> = phrases.iter().take(3).map(|p| format!("“{p}”")).collect();
            reasons.push(format!("mentions {}", quoted.join(", ")));
        }
        // Without the title, require two independent kinds of evidence or
        // two technologies, so one stray word does not make a role.
        let enough = from_title
            || techs.len() >= 2
            || (!techs.is_empty() && !phrases.is_empty())
            || phrases.len() >= 2;
        if enough {
            signals.push(RoleSignal {
                role: terms.role,
                from_title,
                reasons,
            });
        }
    }
    let full_stack_title = ["full stack", "fullstack", "full-stack"]
        .iter()
        .any(|p| Pattern::new(p).find(&title_words).is_some());
    let has = |role: &str| signals.iter().any(|s| s.role == role);
    if full_stack_title || (has("backend") && has("frontend")) {
        let reasons = if full_stack_title {
            vec![format!("title “{}”", title.unwrap_or_default())]
        } else {
            vec!["backend and frontend evidence in the same role".to_owned()]
        };
        signals.push(RoleSignal {
            role: "full stack",
            from_title: full_stack_title,
            reasons,
        });
    }
    signals
}

/// Seniority and ownership signals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipSignal {
    /// `senior`, `staff`, `principal`, `lead`, `management`, `founding`,
    /// `technical leadership`, `mentorship`.
    pub topic: &'static str,
    /// The words as written.
    pub phrase: String,
}

const TITLE_LEVELS: [(&str, &str); 14] = [
    ("co founder", "founding"),
    ("cofounder", "founding"),
    ("founder", "founding"),
    ("founding", "founding"),
    ("cto", "management"),
    ("vp", "management"),
    ("director", "management"),
    ("head", "management"),
    ("manager", "management"),
    ("principal", "principal"),
    ("staff", "staff"),
    ("lead", "lead"),
    ("senior", "senior"),
    ("sr", "senior"),
];

/// The seniority a title states, if any ("Staff Engineer" → `staff`).
pub fn title_level(title: &str) -> Option<OwnershipSignal> {
    let ws = words(title);
    TITLE_LEVELS.iter().find_map(|(pattern, topic)| {
        Pattern::new(pattern)
            .find(&ws)
            .map(|range| OwnershipSignal {
                topic,
                phrase: span_text(title, &ws, &range).to_owned(),
            })
    })
}

const OWNERSHIP_PHRASES: [(&str, &str); 16] = [
    ("led", "technical leadership"),
    ("lead", "technical leadership"),
    ("leading", "technical leadership"),
    ("owned", "technical leadership"),
    ("owning", "technical leadership"),
    ("architected", "technical leadership"),
    ("spearheaded", "technical leadership"),
    ("drove", "technical leadership"),
    ("architectural decision*", "technical leadership"),
    ("mentored", "mentorship"),
    ("mentoring", "mentorship"),
    ("coached", "mentorship"),
    ("hiring", "mentorship"),
    ("founded", "founding"),
    ("co founded", "founding"),
    ("first engineer*", "founding"),
];

/// Ownership phrases in a bullet ("Owned architectural decisions …").
pub fn ownership_in(text: &str) -> Vec<OwnershipSignal> {
    let ws = words(text);
    let mut out: Vec<OwnershipSignal> = Vec::new();
    for (pattern, topic) in OWNERSHIP_PHRASES {
        if out.iter().any(|s| s.topic == topic) {
            continue;
        }
        if let Some(range) = Pattern::new(pattern).find(&ws) {
            out.push(OwnershipSignal {
                topic,
                phrase: span_text(text, &ws, &range).to_owned(),
            });
        }
    }
    out
}

const RESULT_VERBS: [&str; 30] = [
    "built",
    "designed",
    "launched",
    "shipped",
    "created",
    "delivered",
    "led",
    "reduced",
    "increased",
    "improved",
    "cut",
    "grew",
    "saved",
    "migrated",
    "integrated",
    "founded",
    "architected",
    "automated",
    "scaled",
    "won",
    "introduced",
    "rewrote",
    "redesigned",
    "established",
    "doubled",
    "tripled",
    "halved",
    "eliminated",
    "accelerated",
    "implemented",
];

/// Whether a bullet reports a result (an accomplishment) rather than a
/// duty (a responsibility): it starts with a result verb, or quantifies an
/// outcome (a percentage, "2x", "from … to …").
pub fn is_accomplishment(text: &str) -> bool {
    let ws = words(text);
    let starts_with_result = ws
        .first()
        .is_some_and(|w| RESULT_VERBS.contains(&w.lower.as_str()));
    let quantified = text.contains('%')
        || ws.iter().any(|w| {
            w.lower.len() > 1
                && w.lower.ends_with('x')
                && w.lower[..w.lower.len() - 1]
                    .chars()
                    .all(|c| c.is_ascii_digit())
        })
        || (Pattern::new("from").find(&ws).is_some()
            && Pattern::new("to").find(&ws).is_some()
            && text.chars().any(|c| c.is_ascii_digit()));
    starts_with_result || quantified
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(mentions: &[Mention]) -> Vec<&str> {
        mentions.iter().map(|m| m.technology).collect()
    }

    #[test]
    fn finds_technologies_with_word_boundaries() {
        let text = "Owned the event pipeline and led the migration from RabbitMQ to Kafka \
                    on Kubernetes (k8s), using Postgres and React Native.";
        assert_eq!(
            names(&technologies_in(text, false)),
            [
                "RabbitMQ",
                "Kafka",
                "Kubernetes",
                "PostgreSQL",
                "React Native"
            ]
        );
        // English words are not technologies.
        assert!(technologies_in("We go to market swiftly with express delivery", false).is_empty());
        assert!(technologies_in("Series C funding", false).is_empty());
        assert_eq!(
            names(&technologies_in("Rust, C, Go", true)),
            ["Rust", "C", "Go"]
        );
        assert_eq!(technologies_in("postgres", false)[0].matched, "postgres");
    }

    #[test]
    fn canonicalizes_known_technologies() {
        assert_eq!(known_technology("postgres").unwrap().name, "PostgreSQL");
        assert_eq!(known_technology("Next.js").unwrap().name, "Next.js");
        assert_eq!(known_technology("k8s").unwrap().name, "Kubernetes");
        assert!(known_technology("Leadership").is_none());
    }

    #[test]
    fn finds_domains_with_the_triggering_words() {
        let hits = domains_in("Integrated four payment providers and built PIX fraud detection");
        let domains: Vec<&str> = hits.iter().map(|h| h.domain).collect();
        assert!(domains.contains(&"payments"));
        assert!(domains.contains(&"fraud and risk"));
        let payment = hits
            .iter()
            .find(|h| h.domain == "payments" && h.strong)
            .unwrap();
        assert_eq!(payment.phrase, "payment");
        assert_eq!(canonical_domain("devtools"), Some("developer tools"));
        assert_eq!(canonical_domain("gambling"), Some("gambling"));
        assert_eq!(canonical_domain("weather"), None);
    }

    #[test]
    fn role_signals_need_real_evidence() {
        let signals = role_signals(
            Some("Senior Backend Engineer"),
            &["PostgreSQL", "React"],
            &["Built the settlement API"],
        );
        let roles: Vec<&str> = signals.iter().map(|s| s.role).collect();
        assert_eq!(roles, ["backend"], "one frontend technology is not enough");
        assert!(signals[0].from_title);

        let signals = role_signals(
            Some("Software Engineer"),
            &["React", "Next.js", "PostgreSQL", "Node.js"],
            &[],
        );
        let roles: Vec<&str> = signals.iter().map(|s| s.role).collect();
        assert_eq!(roles, ["backend", "frontend", "full stack"]);
        assert!(signals.iter().all(|s| !s.from_title));
    }

    #[test]
    fn seniority_and_ownership() {
        assert_eq!(
            title_level("Staff Software Engineer").unwrap().topic,
            "staff"
        );
        assert_eq!(title_level("Co-Founder & CTO").unwrap().topic, "founding");
        assert_eq!(title_level("Software Engineer II"), None);
        let signals = ownership_in("Mentored three engineers and led the migration");
        let topics: Vec<&str> = signals.iter().map(|s| s.topic).collect();
        assert_eq!(topics, ["technical leadership", "mentorship"]);
    }

    #[test]
    fn classifies_bullets() {
        assert!(is_accomplishment(
            "Designed a rules engine, reducing manual reviews by 40%."
        ));
        assert!(is_accomplishment("Cut p99 latency from 800 ms to 120 ms"));
        assert!(!is_accomplishment(
            "Developed internal services for account onboarding"
        ));
        assert!(!is_accomplishment("Participated in on-call rotations"));
    }
}
