import Link from "next/link";

import { Wordmark } from "@/components/brand";
import { buttonClass } from "@/components/ui";

export default function NotFound() {
  return (
    <main id="main" className="mx-auto max-w-[560px] px-5 py-24">
      <Wordmark size={15} />
      <h1 className="mt-12 text-heading-l">Not here</h1>
      <p className="mt-2.5 text-body-s text-fg-secondary">That page or opportunity doesn&apos;t exist, or it was merged with another listing.</p>
      <Link href="/today" className={buttonClass("secondary", "md", "mt-6 max-sm:h-11")}>
        Back to Today
      </Link>
    </main>
  );
}
