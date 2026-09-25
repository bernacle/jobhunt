import type { Metadata } from "next";
import Link from "next/link";

import { Notice, PageHeader } from "@/components/ui";
import { ApiError, api, load } from "@/lib/api";
import { describeError } from "@/lib/errors";

export const metadata: Metadata = { title: "Confirm your email" };

/** The link in the confirmation email lands here (signed in). */
export default async function ConfirmEmail({ searchParams }: { searchParams: Promise<{ token?: string }> }) {
  const { token } = await searchParams;
  let error: string | null = null;
  if (!token) {
    error = "invalid_confirmation";
  } else {
    try {
      await load(() => api.confirmEmail(token));
    } catch (e) {
      if (!(e instanceof ApiError)) throw e;
      error = e.code;
    }
  }
  const described = error ? describeError(error) : null;
  return (
    <div>
      <PageHeader title={described ? described.title : "Email confirmed"} />
      {described ? (
        <Notice tone="error" role="alert">
          {described.message}
        </Notice>
      ) : (
        <Notice tone="success" role="status">
          JobHunt will email you when strong new matches appear — and only then.
        </Notice>
      )}
      <Link href="/settings#notifications" className="mt-6 inline-block underline">
        Notification settings
      </Link>
    </div>
  );
}
