import { PageHeader, Skeleton, SystemStatus } from "@/components/ui";

/** The shape of Today while it loads: no spinner theatrics. */
export default function Loading() {
  return (
    <div aria-busy="true">
      <PageHeader title="Today" />
      <div className="-mt-5 mb-8 max-sm:-mt-3.5 max-sm:mb-5" aria-live="polite">
        <SystemStatus>Loading today&apos;s list</SystemStatus>
      </div>
      <div className="flex flex-col gap-3">
        <Skeleton />
        <Skeleton />
      </div>
      <div className="mt-11">
        <Skeleton variant="row" />
        <Skeleton variant="row" />
        <Skeleton variant="row" />
      </div>
    </div>
  );
}
