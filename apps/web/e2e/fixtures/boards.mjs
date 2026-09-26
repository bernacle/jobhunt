// Job boards for the end-to-end tests and local development, in
// Greenhouse's Job Board API format (what boards-api.greenhouse.io
// answers). Written for a person like the resume fixture Ana Lima: a Go
// backend engineer in Lisbon who wants small product teams, remote, at
// least USD 120k, and no pure SRE work. The real discovery and
// verification workers read these through fixture-server.mjs; no test
// touches a live job board.

const escape = (html) => html.replaceAll("&", "&amp;").replaceAll("<", "&lt;").replaceAll(">", "&gt;");

const BACKEND = `<p>We are a small product team of 12 engineers building developer tools for payments.</p>
<h3>What you'll do</h3><ul><li>Own backend services in Go and PostgreSQL, with Kafka.</li><li>Ship to production daily with a team that values ownership.</li></ul>
<p>Fully remote.</p>`;
const PLATFORM = `<p>Small team building the platform for developer tooling: Go, Kubernetes, PostgreSQL.</p>
<p>You will own our internal CLI and developer platform. Remote.</p>`;
const SRE = `<p>Site reliability engineering: on-call rotation, incident response, Kubernetes and Terraform operations. Pure SRE role, 24/7 on-call.</p>`;
const MARKETING = `<p>Own our brand campaigns and marketing calendar. Content, SEO and events. No engineering.</p>`;

function job(board, id, title, content, pay, location = "Remote - Worldwide") {
  return {
    id,
    internal_job_id: id,
    title,
    absolute_url: `https://job-boards.greenhouse.io/${board}/jobs/${id}`,
    company_name: board,
    location: { name: location },
    offices: [],
    departments: [{ name: "Engineering" }],
    metadata: null,
    pay_input_ranges: pay
      ? [{ min_cents: pay[0] * 100, max_cents: pay[1] * 100, currency_type: "USD", title: "Annual base salary" }]
      : [],
    content: escape(content),
    first_published: "2026-09-18T09:00:00-04:00",
    updated_at: "2026-09-18T09:00:00-04:00",
    requisition_id: `E2E-${id}`,
    language: "en",
  };
}

/** Board name → { company, jobs }. */
export const BOARDS = {
  ledgerly: { company: "Ledgerly", jobs: [job("ledgerly", 7001001, "Senior Backend Engineer (Go)", BACKEND, [150000, 190000])] },
  toolbox: { company: "Toolbox", jobs: [job("toolbox", 7002001, "Staff Platform Engineer", PLATFORM, [170000, 210000])] },
  paydev: { company: "PayDev", jobs: [job("paydev", 7003001, "Backend Engineer, Payments APIs", BACKEND, [140000, 175000])] },
  northwind: { company: "Northwind Ledger", jobs: [job("northwind", 7004001, "Backend Engineer, Ledger", BACKEND, [150000, 185000])] },
  quanta: { company: "Quanta Tools", jobs: [job("quanta", 7005001, "Senior Backend Engineer, Developer Platform", PLATFORM, [165000, 205000])] },
  harbor: { company: "Harbor", jobs: [job("harbor", 7006001, "Backend Engineer, Billing", BACKEND, [145000, 180000])] },
  cliworks: {
    company: "CLI Works",
    jobs: [job("cliworks", 7007001, "Senior Software Engineer, Developer Tools", PLATFORM, [160000, 200000])],
  },
  opsco: { company: "OpsCo", jobs: [job("opsco", 7008001, "Site Reliability Engineer", SRE, [150000, 180000])] },
  brandly: { company: "Brandly", jobs: [job("brandly", 7009001, "Marketing Manager", MARKETING, null)] },
  // Empty at first; the tests publish its job mid-run (a new opening
  // appearing after the person last looked).
  latecomer: {
    company: "Latecomer",
    jobs: [],
    later: [job("latecomer", 7010001, "Staff Backend Engineer, Payments Platform", BACKEND, [175000, 215000])],
  },
};

/** Recorded real boards (from the source adapters' fixtures) for volume. */
export const RECORDED = { figma: "../../../crates/jobhunt-sources/tests/fixtures/greenhouse/figma.json" };
