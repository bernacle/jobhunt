import { PageHeader, Skeleton, SystemStatus } from "@/components/ui";

/** The shape of Today while it loads (a lead, then peers side by side): no spinner theatrics. */
export default function Loading() {
  return (
    <div aria-busy="true">
      <PageHeader title="Today" />
      <div className="-mt-5 mb-8 max-sm:-mt-3.5 max-sm:mb-5" aria-live="polite">
        <SystemStatus>Loading today&apos;s list</SystemStatus>
      </div>
      <Skeleton />
      <div className="@container mt-11 max-sm:mt-8">
        <div className="grid gap-x-10 @2xl:grid-cols-2 @4xl:grid-cols-3">
          <Skeleton variant="peer" />
          <Skeleton variant="peer" />
          <Skeleton variant="peer" />
        </div>
      </div>
    </div>
  );
}
