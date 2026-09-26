"use client";

import { type KeyboardEvent, type ReactNode, useId, useRef, useState } from "react";

export type StageTab = { id: string; label: string; count: number };

/**
 * Tabs over the application stages (All, Saved, Applied, Interview,
 * Offer): a hairline tab list, arrow keys between tabs, one panel. The
 * groups are rendered on the server; this only chooses which show.
 */
export function StageTabs({ tabs, groups }: { tabs: StageTab[]; groups: { stage: string; node: ReactNode }[] }) {
  const [selected, setSelected] = useState(tabs[0]?.id ?? "all");
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const base = useId();
  const tabId = (id: string) => `${base}-tab-${id}`;
  const panelId = `${base}-panel`;

  const onKeyDown = (e: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const last = tabs.length - 1;
    const to = e.key === "ArrowRight" ? (index === last ? 0 : index + 1) : e.key === "ArrowLeft" ? (index === 0 ? last : index - 1) : e.key === "Home" ? 0 : e.key === "End" ? last : null;
    if (to === null) return;
    e.preventDefault();
    setSelected(tabs[to]!.id);
    refs.current[to]?.focus();
  };

  const shown = selected === "all" ? groups : groups.filter((g) => g.stage === selected);
  return (
    <div>
      <div role="tablist" aria-label="Stages" className="flex gap-6 overflow-x-auto border-b border-line-subtle max-sm:gap-5">
        {tabs.map((tab, i) => {
          const active = tab.id === selected;
          return (
            <button
              key={tab.id}
              ref={(el) => {
                refs.current[i] = el;
              }}
              id={tabId(tab.id)}
              type="button"
              role="tab"
              aria-selected={active}
              aria-controls={panelId}
              tabIndex={active ? 0 : -1}
              onClick={() => setSelected(tab.id)}
              onKeyDown={(e) => onKeyDown(e, i)}
              className={`flex shrink-0 cursor-pointer items-baseline gap-1 pb-2.5 text-ui-m transition-colors duration-[120ms] max-sm:min-h-11 max-sm:items-end ${
                active ? "text-fg shadow-[inset_0_-1px_0_var(--nr-fg-primary)]" : "text-fg-muted hover:text-fg-secondary"
              }`}
            >
              {tab.label}{" "}
              <span className={`ml-1 font-mono text-mono-xs ${active ? "text-fg-secondary" : ""}`}>{tab.count}</span>
            </button>
          );
        })}
      </div>
      <div role="tabpanel" id={panelId} aria-labelledby={tabId(selected)}>
        {shown.map((g) => (
          <div key={g.stage}>{g.node}</div>
        ))}
        {shown.length === 0 && <p className="py-7 text-[14px] text-fg-muted">Nothing at this stage.</p>}
      </div>
    </div>
  );
}
