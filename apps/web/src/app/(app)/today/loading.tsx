export default function Loading() {
  return (
    <div aria-busy="true" aria-live="polite">
      <h1 className="font-serif text-3xl leading-tight tracking-tight sm:text-4xl">Today</h1>
      <p className="mt-6 text-muted">Looking at what&apos;s new for you…</p>
      <div className="mt-6 space-y-3" aria-hidden="true">
        <div className="h-5 w-2/3 rounded bg-sunken" />
        <div className="h-5 w-1/2 rounded bg-sunken" />
      </div>
    </div>
  );
}
