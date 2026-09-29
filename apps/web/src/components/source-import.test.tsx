import { fireEvent, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { SourceImportResult, TallyView } from "@/lib/api-types";

import { violations } from "../../test/axe";
import { GithubImport, LinkedinUpload, RemoveSource, SourceSummary } from "./source-import";

const none: TallyView = { added: 0, updated: 0, unchanged: 0, stale: 0, restored: 0, corroborated: 0 };

function result(overrides: Partial<SourceImportResult>): SourceImportResult {
  return {
    source: "linkedin",
    label: "Basic_LinkedInDataExport.zip",
    first_import: true,
    unchanged: false,
    read: ["positions (2 rows)", "skills (4 rows)"],
    skipped: ["7 other files in the export, never opened (messages, connections, contacts, …)"],
    experiences: { ...none, added: 1, corroborated: 1 },
    projects: none,
    education: none,
    skills: { ...none, added: 2 },
    claims: { ...none, added: 9, corroborated: 3 },
    kept_confirmed: 0,
    kept_rejected: 0,
    reconfirm: 0,
    stale_confirmed: 0,
    preserved_edits: 0,
    conflicts: 1,
    notes: [],
    problems: [],
    needs_review: 4,
    profile: {} as SourceImportResult["profile"],
    ...overrides,
  };
}

describe("SourceSummary", () => {
  it("says what was read, what was never opened, and what waits for review", async () => {
    const { container } = render(<SourceSummary result={result({})} />);
    const status = screen.getByRole("status");
    expect(within(status).getByText("LinkedIn export imported")).toBeInTheDocument();
    expect(status).toHaveTextContent("Read: positions (2 rows), skills (4 rows).");
    expect(status).toHaveTextContent("Not used: 7 other files in the export, never opened");
    expect(status).toHaveTextContent("Experiences: 1 added, 1 already in your profile, now also backed by this source");
    expect(status).toHaveTextContent("1 position is dated differently than in another source");
    expect(within(status).getByRole("link", { name: "Review 4 claims" })).toHaveAttribute("href", "/profile/review");
    expect(await violations(container)).toEqual([]);
  });

  it("reports an unchanged re-import and partial GitHub reads plainly", () => {
    const { rerender } = render(<SourceSummary result={result({ unchanged: true, needs_review: 0 })} />);
    expect(screen.getByText("Same LinkedIn export as before")).toBeInTheDocument();
    expect(screen.queryByRole("link")).not.toBeInTheDocument();
    rerender(
      <SourceSummary
        result={result({
          source: "github",
          label: "github.com/octocat",
          first_import: false,
          problems: ["languages of octocat/lox unavailable (HTTP 502); used its primary language"],
        })}
      />,
    );
    expect(screen.getByText("GitHub re-imported")).toBeInTheDocument();
    expect(screen.getByText("Some of it couldn't be read")).toBeInTheDocument();
    expect(screen.getByText(/octocat\/lox unavailable/)).toBeInTheDocument();
  });
});

describe("LinkedinUpload", () => {
  it("explains what is read and never opened, and sends the chosen file", async () => {
    const action = vi.fn(async () => ({ result: result({}) }));
    const { container } = render(<LinkedinUpload action={action} imported={false} />);
    expect(screen.getByText(/Messages, connections and contacts are never opened/)).toBeInTheDocument();
    expect(screen.getByText(/No LinkedIn password/)).toBeInTheDocument();
    const input = screen.getByLabelText("Your LinkedIn data export");
    expect(input).toHaveAttribute("accept", ".zip,.csv,application/zip,text/csv");
    await userEvent.upload(input, new File(["Company Name,Title\n"], "Positions.csv", { type: "text/csv" }));
    // jsdom does not count uploaded files for `required`; submit directly.
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByText("LinkedIn export imported")).toBeInTheDocument();
    expect(action).toHaveBeenCalledOnce();
    const form = (action.mock.calls[0] as unknown as [unknown, FormData])[1];
    // jsdom's FormData drops the file's name; the browser test uploads a real one.
    expect(form.has("export")).toBe(true);
    expect(await violations(container)).toEqual([]);
  });

  it("shows why a file was refused", async () => {
    const action = vi.fn(async () => ({ error: { title: "That file couldn't be read", message: "Nothing was imported." } }));
    render(<LinkedinUpload action={action} imported />);
    const input = screen.getByLabelText("A newer LinkedIn export");
    await userEvent.upload(input, new File(["x"], "notes.zip", { type: "application/zip" }));
    expect(screen.getByRole("button", { name: "Re-import" })).toBeInTheDocument();
    fireEvent.submit(input.closest("form")!);
    expect(await screen.findByRole("alert")).toHaveTextContent("That file couldn't be read");
  });
});

describe("GithubImport", () => {
  it("imports a username, public data only", async () => {
    const action = vi.fn(async () => ({ result: result({ source: "github", label: "github.com/octocat" }) }));
    const { container } = render(<GithubImport action={action} />);
    expect(screen.getByText(/Public data only/)).toBeInTheDocument();
    expect(screen.getByText(/organizations aren.t read as employers/)).toBeInTheDocument();
    await userEvent.type(screen.getByLabelText("GitHub username or profile URL"), "octocat");
    await userEvent.click(screen.getByRole("button", { name: "Import" }));
    expect(await screen.findByText("GitHub imported")).toBeInTheDocument();
    const form = (action.mock.calls[0] as unknown as [unknown, FormData])[1];
    expect(form.get("username")).toBe("octocat");
    expect(await violations(container)).toEqual([]);
  });

  it("offers the imported account again", () => {
    render(<GithubImport action={vi.fn()} login="octocat" />);
    expect(screen.getByLabelText("GitHub username or profile URL")).toHaveValue("octocat");
    expect(screen.getByRole("button", { name: "Re-import" })).toBeInTheDocument();
  });
});

describe("RemoveSource", () => {
  it("asks once, says what stays, then removes", async () => {
    const remove = vi.fn(async () => ({ ok: true as const, data: {} as never }));
    const { container } = render(<RemoveSource source="linkedin" label="LinkedIn export" remove={remove} />);
    await userEvent.click(screen.getByRole("button", { name: "Remove LinkedIn export" }));
    const group = screen.getByRole("group", { name: "Remove LinkedIn export" });
    expect(group).toHaveTextContent("so do your confirmations and rejections");
    expect(remove).not.toHaveBeenCalled();
    expect(await violations(container)).toEqual([]);
    await userEvent.click(within(group).getByRole("button", { name: "Cancel" }));
    expect(screen.queryByRole("group")).not.toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Remove LinkedIn export" }));
    await userEvent.click(within(screen.getByRole("group")).getByRole("button", { name: "Remove LinkedIn export" }));
    expect(remove).toHaveBeenCalledWith("linkedin");
  });

  it("shows a failure", async () => {
    const remove = vi.fn(async () => ({ ok: false as const, code: "conflict", title: "Something changed", message: "Try again." }));
    render(<RemoveSource source="github" label="GitHub" remove={remove} />);
    await userEvent.click(screen.getByRole("button", { name: "Remove GitHub" }));
    await userEvent.click(within(screen.getByRole("group")).getByRole("button", { name: "Remove GitHub" }));
    expect(await screen.findByRole("alert")).toHaveTextContent("Something changed. Try again.");
  });
});
