"use client";

import Link from "next/link";
import { useState } from "react";

import type { PipelineStage } from "@/lib/api-types";
import { STAGE_LABEL } from "@/lib/format";

import { FeedbackActions, type FeedbackActionsProps, type Outcome, OutcomeLine } from "./feedback-actions";
import { textLinkClass } from "./ui";

/**
 * The opportunity page's actions: the same as on Today while undecided,
 * the stage once it's in Applications. Under the key facts on wide
 * screens, where Today's lead has them (right-aligned, primary last); a
 * bar within thumb reach at the bottom on phones and tablets.
 */
export function DetailActions({
  id,
  title,
  company,
  stage,
  actions,
}: {
  id: string;
  title: string;
  company: string;
  stage: PipelineStage;
  actions: FeedbackActionsProps["actions"];
}) {
  const [outcome, setOutcome] = useState<Outcome | null>(null);
  let content;
  if (outcome) {
    content = (
      <div role="status" className="text-[13px] text-fg-secondary">
        <OutcomeLine outcome={outcome} />
      </div>
    );
  } else if (stage !== "unseen" && stage !== "seen") {
    content = (
      <p className="text-[13px] text-fg-secondary">
        <span className="text-fg">{STAGE_LABEL[stage]}.</span>{" "}
        <Link href="/applications" className={textLinkClass}>
          Manage it in Applications
        </Link>
      </p>
    );
  } else {
    content = <FeedbackActions id={id} title={title} company={company} actions={actions} onDone={setOutcome} variant="detail" />;
  }
  return (
    <div
      className={
        "flex justify-end max-lg:fixed max-lg:inset-x-0 max-lg:bottom-0 max-lg:z-30 " +
        "max-lg:border-t max-lg:border-line-subtle max-lg:bg-ground max-lg:px-4 max-lg:pt-2.5 max-lg:pb-[calc(16px+env(safe-area-inset-bottom))] " +
        "max-sm:block md:max-lg:px-10"
      }
    >
      {content}
    </div>
  );
}
