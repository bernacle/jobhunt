import type { Metadata } from "next";

import { Notice } from "@/components/ui";
import { config } from "@/lib/config";
import { safeNext } from "@/lib/redirects";

export const metadata: Metadata = { title: "Sign in" };

const ERRORS: Record<string, string> = {
  expired: "The sign-in took too long. Try again.",
  denied: "Sign-in was cancelled.",
  provider: "The sign-in service didn't answer. Try again in a moment.",
  name: "Enter a name.",
};

export default async function SignIn({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | undefined>>;
}) {
  const params = await searchParams;
  const next = safeNext(params.next);
  const dev = config().authMode === "dev";
  return (
    <main id="main" className="mx-auto flex min-h-dvh max-w-md flex-col justify-center px-4 py-16">
      <p className="font-serif text-2xl">JobHunt</p>
      <h1 className="mt-10 font-serif text-4xl leading-tight tracking-tight">
        The few jobs worth your time.
      </h1>
      <p className="mt-4 text-muted">
        JobHunt reads company job boards, checks each listing at the source, and shows you only the openings that fit
        what you want — with why, and what to watch out for.
      </p>
      <div className="mt-8 space-y-3">
        {params.expired && <Notice tone="caution" role="status" title="Your session ended">Sign in again to pick up where you left off.</Notice>}
        {params.signed_out && <Notice role="status" title={params.signed_out === "everywhere" ? "Signed out everywhere" : "Signed out"} />}
        {params.error && <Notice tone="error" role="alert" title={ERRORS[params.error] ?? "Sign-in failed. Try again."} />}
      </div>
      {dev ? (
        <form action="/auth/dev" method="post" className="mt-8 space-y-3">
          <input type="hidden" name="next" value={next} />
          <label htmlFor="name" className="block text-sm font-medium">
            Your name <span className="font-normal text-muted">(development sign-in)</span>
          </label>
          <input
            id="name"
            name="name"
            required
            autoComplete="username"
            className="w-full rounded-md border border-line-strong bg-surface px-3 py-2"
          />
          <button type="submit" className="w-full rounded-md bg-accent px-4 py-2.5 font-medium text-accent-ink hover:opacity-90">
            Continue
          </button>
        </form>
      ) : (
        <a
          href={`/auth/login?next=${encodeURIComponent(next)}`}
          className="mt-8 block rounded-md bg-accent px-4 py-2.5 text-center font-medium text-accent-ink hover:opacity-90"
        >
          Sign in
        </a>
      )}
      <p className="mt-6 text-xs text-muted">
        One account for the web, the <code className="font-mono">jobhunt</code> command and AI assistants.
      </p>
    </main>
  );
}
