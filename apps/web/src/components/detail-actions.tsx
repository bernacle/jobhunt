"use client";

import Link from "next/link";
import { useState } from "react";

import type { PipelineStage } from "@/lib/api-types";
import { STAGE_LABEL } from "@/lib/format";

import { FeedbackActions, type FeedbackActionsProps, type Outcome, OutcomeLine } from "./feedback-actions";

/** Actions on the detail page: the same as the card while undecided. */
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
  if (outcome) {
    return (
      <div role="status" className="text-sm text-muted">
        <OutcomeLine outcome={outcome} />
      </div>
    );
  }
  if (stage !== "unseen" && stage !== "seen") {
    return (
      <p className="text-sm">
        {STAGE_LABEL[stage]}.{" "}
        <Link href="/applications" className="text-muted underline">
          Manage it in Applications
        </Link>
      </p>
    );
  }
  return <FeedbackActions id={id} title={title} company={company} actions={actions} onDone={setOutcome} />;
}
