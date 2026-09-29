// A local stand-in for Greenhouse's Job Board API and hosted job pages,
// serving fixtures/boards.mjs and recorded boards, and for the few GitHub
// REST endpoints evidence imports read (under /github, for the fictional
// account `analima`). Discovery, verification and GitHub imports are
// pointed here with JOBHUNT_DISCOVERY_ENDPOINT, JOBHUNT_VERIFY_ENDPOINT and
// JOBHUNT_GITHUB_ENDPOINT.
import { readFileSync } from "node:fs";
import http from "node:http";

import { BOARDS, RECORDED } from "./fixtures/boards.mjs";

export function loadBoards() {
  const boards = {};
  for (const [name, board] of Object.entries(BOARDS)) boards[name] = [...board.jobs];
  for (const [name, path] of Object.entries(RECORDED)) {
    boards[name] = JSON.parse(readFileSync(new URL(path, import.meta.url), "utf8")).jobs;
  }
  return boards;
}

export function startFixtureServer(port) {
  const boards = loadBoards();
  const server = http.createServer((request, response) => {
    const url = new URL(request.url ?? "/", "http://fixtures");
    const send = (status, body, type = "application/json") => {
      response.writeHead(status, { "content-type": type });
      response.end(typeof body === "string" ? body : JSON.stringify(body));
    };
    const parts = url.pathname.split("/").filter(Boolean);
    // POST /__fixtures/publish/<board>: its later jobs appear on the board.
    if (request.method === "POST" && parts[0] === "__fixtures" && parts[1] === "publish") {
      const later = BOARDS[parts[2]]?.later ?? [];
      for (const job of later) if (!boards[parts[2]].some((j) => j.id === job.id)) boards[parts[2]].push(job);
      return send(200, { published: later.length });
    }
    // /v1/boards/<board>/jobs[/<id>]
    if (parts[0] === "v1" && parts[1] === "boards" && parts[3] === "jobs") {
      const jobs = boards[parts[2]];
      if (!jobs) return send(404, { error: "board not found" });
      if (parts.length === 4) return send(200, { jobs, meta: { total: jobs.length } });
      const job = jobs.find((j) => String(j.id) === parts[4]);
      return job ? send(200, job) : send(404, { error: "job not found" });
    }
    // /<board>/jobs/<id>: the hosted job page with the application form.
    if (parts.length === 3 && parts[1] === "jobs") {
      const job = boards[parts[0]]?.find((j) => String(j.id) === parts[2]);
      if (!job) return send(404, "<html><body>Not found</body></html>", "text/html");
      return send(200, `<html><body><h1>${job.title}</h1><form id="application_form"></form></body></html>`, "text/html");
    }
    // GitHub's REST API, for the fictional account in the resume fixture.
    if (parts[0] === "github") {
      const route = parts.slice(1).join("/");
      if (route === "users/analima") {
        return send(200, { login: "analima", html_url: "https://github.com/analima", type: "User", public_repos: 2 });
      }
      if (route === "users/analima/repos") {
        const repo = (id, name, fork) => ({
          id,
          name,
          full_name: `analima/${name}`,
          owner: { login: "analima" },
          html_url: `https://github.com/analima/${name}`,
          description: "A linter for double-entry ledgers",
          fork,
          archived: false,
          is_template: false,
          private: false,
          size: 300,
          stargazers_count: 300,
          forks_count: 12,
          language: "Go",
          topics: ["accounting"],
          created_at: "2022-02-01T00:00:00Z",
          pushed_at: new Date().toISOString(),
        });
        return send(200, [repo(10, "ledgerlint", false), repo(11, "kubernetes", true)]);
      }
      if (route === "users/analima/orgs") return send(200, []);
      return send(404, { message: "Not Found" });
    }
    if (url.pathname === "/healthz") return send(200, { status: "ok" });
    return send(404, { error: "not a fixture" });
  });
  return new Promise((resolve) => server.listen(port, "127.0.0.1", () => resolve(server)));
}

if (import.meta.url === `file://${process.argv[1]}`) {
  const port = Number(process.env.PORT ?? 4010);
  await startFixtureServer(port);
  console.log(`fixture job boards on http://127.0.0.1:${port}`);
}
