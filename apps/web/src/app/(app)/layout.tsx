import Link from "next/link";
import type { ReactNode } from "react";

import { MainNav } from "@/components/nav";
import { getSession } from "@/lib/session";

export default async function AppLayout({ children }: { children: ReactNode }) {
  const session = await getSession();
  return (
    <div className="mx-auto flex min-h-dvh max-w-3xl flex-col px-4 sm:px-6">
      <header className="flex flex-col gap-3 border-b border-line py-4 sm:flex-row sm:items-center sm:justify-between">
        <div className="flex items-center justify-between gap-4">
          <Link href="/today" className="font-serif text-xl tracking-tight">
            JobHunt
          </Link>
          <div className="flex items-center gap-3 text-sm sm:hidden">
            <Link href="/settings" className="text-muted hover:text-ink">
              Settings
            </Link>
          </div>
        </div>
        <div className="flex items-center justify-between gap-4">
          <MainNav />
          <div className="hidden items-center gap-3 text-sm sm:flex">
            <Link href="/settings" className="text-muted hover:text-ink">
              Settings
            </Link>
            <form action="/auth/signout" method="post">
              <button type="submit" className="text-muted hover:text-ink" title={session?.email ?? session?.name ?? undefined}>
                Sign out
              </button>
            </form>
          </div>
        </div>
      </header>
      <main id="main" className="flex-1 py-8 sm:py-10">
        {children}
      </main>
      <footer className="border-t border-line py-6 text-xs text-muted">
        JobHunt checks job boards for you and keeps only what&apos;s worth your time.{" "}
        <Link href="/settings#assistant" className="underline">
          Use it from your AI assistant
        </Link>
        .
      </footer>
    </div>
  );
}
