import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { violations } from "../../test/axe";
import { BrandMark, Wordmark } from "./brand";
import { Sidebar, TabBar, TopBar } from "./nav";
import { StageTabs } from "./stage-tabs";
import { ThemeControl } from "./theme-control";

let pathname = "/today";
vi.mock("next/navigation", () => ({ usePathname: () => pathname }));

describe("brand", () => {
  it("is the lowercase narrow wordmark, with a decorative mark", () => {
    const { container } = render(<Wordmark />);
    expect(container).toHaveTextContent(/^narrow$/);
    expect(container.querySelector("svg")).toHaveAttribute("aria-hidden", "true");
    expect(container.textContent).not.toMatch(/jobhunt/i);
  });

  it("draws the mark as ink with the cutout as a hole", () => {
    const { container } = render(<BrandMark size={18} />);
    const path = container.querySelector("path")!;
    expect(path).toHaveAttribute("fill-rule", "evenodd");
    expect(path).toHaveAttribute("fill", "var(--nr-mark)");
  });
});

describe("navigation", () => {
  beforeEach(() => {
    pathname = "/today";
  });

  it("has Today, Applications, then Preferences, Profile and Settings, marking the current one", async () => {
    const { container } = render(<Sidebar account="ana@example.com" />);
    const nav = screen.getByRole("navigation", { name: "Main" });
    expect(within(nav).getAllByRole("link").map((l) => l.textContent)).toEqual(["Today", "Applications", "Preferences", "Profile", "Settings"]);
    expect(within(nav).getByRole("link", { name: "Today" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: /^narrow\s*, Today$/ })).toHaveAttribute("href", "/today");
    expect(await violations(container)).toEqual([]);
  });

  it("keeps Today current on an opportunity, which has its own way back", () => {
    pathname = "/opportunities/opp_1";
    render(<Sidebar />);
    expect(screen.getByRole("link", { name: "Today" })).toHaveAttribute("aria-current", "page");
    const { container } = render(<TabBar />);
    expect(container).toBeEmptyDOMElement();
  });

  it("on phones: four tabs at the bottom, Settings reachable from the top bar", () => {
    pathname = "/profile";
    render(
      <>
        <TopBar />
        <TabBar />
      </>,
    );
    const tabs = screen.getByRole("navigation", { name: "Main" });
    expect(within(tabs).getAllByRole("link").map((l) => l.textContent)).toEqual(["Today", "Applications", "Preferences", "Profile"]);
    expect(within(tabs).getByRole("link", { name: "Profile" })).toHaveAttribute("aria-current", "page");
    expect(screen.getByRole("link", { name: "Settings" })).toHaveAttribute("href", "/settings");
  });
});

describe("StageTabs", () => {
  const tabs = [
    { id: "all", label: "All", count: 2 },
    { id: "saved", label: "Saved", count: 1 },
    { id: "applied", label: "Applied", count: 1 },
  ];
  const groups = [
    { stage: "saved", node: <p>Saved group</p> },
    { stage: "applied", node: <p>Applied group</p> },
  ];

  it("is a tab list over the real stages, with arrow keys", async () => {
    const { container } = render(<StageTabs tabs={tabs} groups={groups} />);
    expect(screen.getByRole("tab", { name: "All 2" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent("Saved groupApplied group");
    screen.getByRole("tab", { name: "All 2" }).focus();
    await userEvent.keyboard("{ArrowRight}");
    expect(screen.getByRole("tab", { name: "Saved 1" })).toHaveFocus();
    expect(screen.getByRole("tab", { name: "Saved 1" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tabpanel")).toHaveTextContent(/^Saved group$/);
    await userEvent.keyboard("{End}");
    expect(screen.getByRole("tabpanel")).toHaveTextContent(/^Applied group$/);
    expect(await violations(container)).toEqual([]);
  });
});

describe("ThemeControl", () => {
  it("pins dark or light on <html>, or follows the system", async () => {
    render(<ThemeControl initial="system" />);
    await userEvent.click(screen.getByLabelText("Light"));
    expect(document.documentElement.dataset.theme).toBe("light");
    expect(document.cookie).toContain("narrow_theme=light");
    await userEvent.click(screen.getByLabelText("Dark"));
    expect(document.documentElement.dataset.theme).toBe("dark");
    await userEvent.click(screen.getByLabelText("System"));
    expect(document.documentElement.hasAttribute("data-theme")).toBe(false);
    expect(document.cookie).not.toContain("narrow_theme");
  });
});
