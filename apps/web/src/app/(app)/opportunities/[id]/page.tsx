import type { Metadata } from "next";
import { notFound } from "next/navigation";

import { putAside, recordFeedback } from "@/app/actions";
import { OpportunityBrief } from "@/components/opportunity-brief";
import { ApiError, api, load } from "@/lib/api";
import type { ApplicationContext, JobDetail } from "@/lib/api-types";

export const metadata: Metadata = { title: "Opportunity" };

async function detail(id: string): Promise<JobDetail> {
  try {
    return await load(() => api.opportunity(id));
  } catch (error) {
    if (error instanceof ApiError && ["unknown_opportunity", "invalid_arguments", "ambiguous_id"].includes(error.code)) {
      notFound();
    }
    throw error;
  }
}

export default async function OpportunityPage({ params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  const job = await detail(id);
  // The evidence seam: read-only, and only when there is a profile.
  const context: ApplicationContext | null = job.decision ? await load(() => api.applicationContext(id)).catch(() => null) : null;
  return <OpportunityBrief job={job} context={context} actions={{ feedback: recordFeedback, putAside }} />;
}
