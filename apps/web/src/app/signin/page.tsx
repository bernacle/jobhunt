import type { Metadata } from "next";

import { Wordmark } from "@/components/brand";
import { Notice, buttonClass, inputClass, labelClass } from "@/components/ui";
import { config } from "@/lib/config";
import { safeNext } from "@/lib/redirects";

export const metadata: Metadata = { title: "Sign in" };

const ERRORS: Record<string, string> = {
  expired: "The sign-in took too long. Try again.",
  denied: "Sign-in was cancelled.",
  provider: "The sign-in service didn't answer. Try again in a moment.",
  name: "Enter a name.",
};

// What Narrow does, step by step. Described, never counted: no numbers
// here stand for real production volumes.
const STEPS: [string, string][] = [
  ["Discover", "Reads company job boards and careers pages for the kinds of role you want."],
  ["Verify", "Checks that each listing is still open at the source, and confirms pay where it's published."],
  ["Check eligibility", "Location, work authorization and time zone, against your profile."],
  ["Rank for you", "Ordered by what you told Narrow and what it learned from your decisions. Only the few worth reviewing reach Today."],
];

/**
 * The signed-out surface: what Narrow is, how it checks, and the way in.
 */
export default async function SignIn({
  searchParams,
}: {
  searchParams: Promise<Record<string, string | undefined>>;
}) {
  const params = await searchParams;
  const next = safeNext(params.next);
  const dev = config().authMode === "dev";
  return (
    <div className="min-h-dvh bg-inset">
      <div className="mx-auto max-w-[1120px]">
        <header className="flex items-center justify-between gap-3 px-10 py-[18px] max-sm:px-5">
          <Wordmark size={15} />
          <nav aria-label="Site">
            <a href="#how" className="flex min-h-11 items-center text-ui-m text-fg-secondary hover:text-fg">
              How Narrow checks
            </a>
          </nav>
        </header>
        <main id="main" className="px-10 pt-[72px] max-sm:px-5 max-sm:pt-10">
          <section aria-labelledby="hero" className="max-w-[820px]">
            <h1
              id="hero"
              className="text-[56px] leading-[1.03] font-semibold tracking-[-0.035em] text-pretty [font-stretch:92%] max-md:text-[44px] max-sm:text-[36px] max-sm:leading-[1.08]"
            >
              The few jobs worth your time.
            </h1>
            <p className="mt-[22px] max-w-[580px] text-[17px] leading-[1.6] text-pretty text-fg-secondary max-sm:text-[16px]">
              Narrow reads company job boards, checks each listing at the source and against where you can work, and shows you only the few
              openings worth reviewing, with why and what to watch out for.
            </p>
            <div className="mt-6 max-w-[420px] space-y-3 empty:hidden">
              {params.expired && (
                <Notice tone="caution" role="status" title="Your session ended">
                  Sign in again to pick up where you left off.
                </Notice>
              )}
              {params.signed_out && <Notice role="status" title={params.signed_out === "everywhere" ? "Signed out everywhere" : "Signed out"} />}
              {params.error && <Notice tone="error" role="alert" title={ERRORS[params.error] ?? "Sign-in failed. Try again."} />}
            </div>
            {dev ? (
              <form action="/auth/dev" method="post" className="mt-7 max-w-[420px]">
                <input type="hidden" name="next" value={next} />
                <label htmlFor="name" className={labelClass}>
                  Your name <span className="text-fg-muted">(development sign-in)</span>
                </label>
                <div className="flex gap-2 max-sm:flex-col">
                  <input id="name" name="name" required autoComplete="username" className={`${inputClass} h-10`} />
                  <button type="submit" className={buttonClass("primary", "lg", "shrink-0 max-sm:h-11")}>
                    Continue
                  </button>
                </div>
              </form>
            ) : (
              <div className="mt-7 flex flex-wrap gap-2.5 max-sm:grid">
                <a href={`/auth/login?next=${encodeURIComponent(next)}`} className={buttonClass("primary", "lg", "max-sm:h-11")}>
                  Sign in
                </a>
                <a href="#how" className={buttonClass("secondary", "lg", "max-sm:h-11")}>
                  See how Narrow checks
                </a>
              </div>
            )}
          </section>

          <section
            id="how"
            aria-labelledby="how-heading"
            className="mt-20 grid scroll-mt-6 grid-cols-[repeat(auto-fit,minmax(200px,1fr))] gap-6 border-t border-line-subtle pt-12 pb-14 max-sm:mt-14"
          >
            <h2 id="how-heading" className="sr-only">
              How Narrow checks
            </h2>
            {STEPS.map(([title, text], i) => (
              <div key={title}>
                <p aria-hidden="true" className="font-mono text-[12px] text-fg-muted">
                  {String(i + 1).padStart(2, "0")}
                </p>
                <h3 className="mt-2.5 text-[15px] font-semibold">{title}</h3>
                <p className="mt-1 text-[13px] leading-normal text-fg-secondary">{text}</p>
              </div>
            ))}
          </section>

          <footer className="flex flex-wrap justify-between gap-2 border-t border-line-subtle py-6 text-caption text-fg-muted">
            <p>One account for the web, the command line and MCP clients.</p>
            <p className="font-mono text-mono-xs">narrow.fyi</p>
          </footer>
        </main>
      </div>
    </div>
  );
}
