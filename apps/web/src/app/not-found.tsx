import Link from "next/link";

export default function NotFound() {
  return (
    <main id="main" className="mx-auto max-w-md px-4 py-24 text-center">
      <h1 className="font-serif text-3xl">Not here</h1>
      <p className="mt-3 text-muted">That page or opportunity doesn&apos;t exist.</p>
      <Link href="/today" className="mt-6 inline-block underline">
        Back to Today
      </Link>
    </main>
  );
}
