import type { ReactNode } from "react";

import { Sidebar, TabBar, TopBar } from "@/components/nav";
import { getSession } from "@/lib/session";

/**
 * The signed-in shell: a sidebar on wide screens; on phones and tablets a
 * top bar (wordmark, Settings) and a tab bar within thumb reach. Pages
 * set their own measure inside the 920px content column.
 */
export default async function AppLayout({ children }: { children: ReactNode }) {
  const session = await getSession();
  return (
    <div className="min-h-dvh lg:flex">
      <Sidebar account={session?.email ?? session?.name} />
      <div className="min-w-0 flex-1">
        <TopBar />
        <main id="main" className="px-5 pt-6 pb-28 md:px-10 md:pt-8 lg:px-16 lg:pt-11 lg:pb-24">
          <div className="max-w-[920px]">{children}</div>
        </main>
      </div>
      <TabBar />
    </div>
  );
}
