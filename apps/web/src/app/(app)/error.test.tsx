import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";

import { violations } from "../../../test/axe";
import PageError from "./error";

const refresh = vi.fn();
vi.mock("next/navigation", () => ({ useRouter: () => ({ refresh }) }));

function status(reachable: boolean) {
  vi.stubGlobal("fetch", vi.fn(async () => new Response(JSON.stringify({ reachable }))));
}

afterEach(() => {
  vi.unstubAllGlobals();
  vi.spyOn(console, "error").mockRestore();
});

describe("a page that failed to load", () => {
  it("says Narrow can't be reached when the API doesn't answer, and only offers to try again", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    status(false);
    const reset = vi.fn();
    const { container } = render(<PageError error={Object.assign(new Error("stripped in production"), { digest: "123" })} reset={reset} />);
    expect(await screen.findByRole("heading", { level: 1, name: "We can't reach Narrow right now" })).toBeInTheDocument();
    expect(screen.getByText("Decisions you made before this are already saved. Try again in a moment.")).toBeInTheDocument();
    // Nothing is claimed to be queued or kept in the browser.
    expect(container.textContent).not.toMatch(/queue|offline|sync later|stored in your browser/i);
    expect(screen.getAllByRole("button").map((b) => b.textContent)).toEqual(["Try again"]);
    await userEvent.click(screen.getByRole("button", { name: "Try again" }));
    expect(refresh).toHaveBeenCalled();
    expect(reset).toHaveBeenCalled();
    expect(await violations(container)).toEqual([]);
  });

  it("stays generic when the API answers", async () => {
    vi.spyOn(console, "error").mockImplementation(() => {});
    status(true);
    render(<PageError error={new Error("boom")} reset={vi.fn()} />);
    expect(await screen.findByRole("heading", { level: 1, name: "Something went wrong" })).toBeInTheDocument();
  });
});
