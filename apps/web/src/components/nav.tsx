"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

import { Wordmark } from "./brand";

type Item = { href: string; label: string };

const PRODUCT: Item[] = [
  { href: "/today", label: "Today" },
  { href: "/applications", label: "Applications" },
];

const YOU: Item[] = [
  { href: "/preferences", label: "Preferences" },
  { href: "/profile", label: "Profile" },
  { href: "/settings", label: "Settings" },
];

// The phone's tab bar: the four places of the product. Settings is in the
// top bar (and at the end of Profile).
const TABS: Item[] = [...PRODUCT, YOU[0]!, YOU[1]!];

function isCurrent(pathname: string, href: string): boolean {
  if (pathname === href || pathname.startsWith(`${href}/`)) return true;
  // An opportunity is opened from Today.
  return href === "/today" && pathname.startsWith("/opportunities");
}

function SidebarItem({ item, pathname }: { item: Item; pathname: string }) {
  const current = isCurrent(pathname, item.href);
  return (
    <li>
      <Link
        href={item.href}
        aria-current={current ? "page" : undefined}
        className={`flex h-[30px] items-center rounded-md px-2 text-ui-m transition-colors duration-[120ms] ${
          current ? "bg-selected text-fg" : "text-fg-secondary hover:bg-ground-hover hover:text-fg-body"
        }`}
      >
        {item.label}
      </Link>
    </li>
  );
}

/** Wide screens: a quiet sidebar, the wordmark, then the places. */
export function Sidebar({ account }: { account?: string }) {
  const pathname = usePathname();
  return (
    <div className="sticky top-0 hidden h-dvh w-[220px] shrink-0 flex-col border-r border-line-subtle px-3 py-5 lg:flex">
      <Link href="/today" className="self-start rounded-sm px-2 pb-5">
        <Wordmark size={15} />
        <span className="sr-only">, Today</span>
      </Link>
      <nav aria-label="Main" className="flex flex-col">
        <ul className="flex flex-col gap-0.5">
          {PRODUCT.map((item) => (
            <SidebarItem key={item.href} item={item} pathname={pathname} />
          ))}
        </ul>
        <p className="px-2 pt-3.5 pb-1.5 text-[11px] font-medium text-fg-muted">You</p>
        <ul className="flex flex-col gap-0.5">
          {YOU.map((item) => (
            <SidebarItem key={item.href} item={item} pathname={pathname} />
          ))}
        </ul>
      </nav>
      {account && (
        <p className="mt-auto truncate px-2 font-mono text-mono-xs text-fg-muted" title={account}>
          {account}
        </p>
      )}
    </div>
  );
}

/** Phones and tablets: the wordmark, and a clear way to Settings. */
export function TopBar() {
  const pathname = usePathname();
  const onSettings = isCurrent(pathname, "/settings");
  // The opportunity page has its own header with the way back to Today.
  if (pathname.startsWith("/opportunities")) return null;
  return (
    <header className="flex h-[52px] items-center justify-between border-b border-line-subtle pr-2 pl-5 md:pl-10 lg:hidden">
      <Link href="/today" className="rounded-sm">
        <Wordmark size={15} />
        <span className="sr-only">, Today</span>
      </Link>
      <Link
        href="/settings"
        aria-current={onSettings ? "page" : undefined}
        className={`flex min-h-11 items-center rounded-md px-3 text-ui-m ${onSettings ? "text-fg" : "text-fg-secondary hover:text-fg"}`}
      >
        Settings
      </Link>
    </header>
  );
}

/**
 * Phones and tablets: the four places at the bottom, within thumb reach.
 * The opportunity page has its own action bar there instead.
 */
export function TabBar() {
  const pathname = usePathname();
  if (pathname.startsWith("/opportunities")) return null;
  return (
    <nav
      aria-label="Main"
      className="fixed inset-x-0 bottom-0 z-30 border-t border-line-subtle bg-ground pb-[env(safe-area-inset-bottom)] lg:hidden"
    >
      <ul className="grid h-16 grid-cols-4">
        {TABS.map((item) => {
          const current = isCurrent(pathname, item.href);
          return (
            <li key={item.href} className="flex">
              <Link
                href={item.href}
                aria-current={current ? "page" : undefined}
                className={`flex flex-1 items-center justify-center text-[12.5px] font-medium ${current ? "text-fg" : "text-fg-muted hover:text-fg-secondary"}`}
              >
                {item.label}
              </Link>
            </li>
          );
        })}
      </ul>
    </nav>
  );
}
