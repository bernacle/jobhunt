"use client";

import Link from "next/link";
import { usePathname } from "next/navigation";

const ITEMS = [
  { href: "/today", label: "Today" },
  { href: "/applications", label: "Applications" },
  { href: "/preferences", label: "Preferences" },
  { href: "/profile", label: "Profile" },
];

/** The four places of the product. Nothing else competes for attention. */
export function MainNav() {
  const pathname = usePathname();
  return (
    <nav aria-label="Main" className="-mx-1 overflow-x-auto">
      <ul className="flex gap-1 whitespace-nowrap">
        {ITEMS.map((item) => {
          const current = pathname === item.href || pathname.startsWith(`${item.href}/`) ||
            (item.href === "/today" && pathname.startsWith("/opportunities"));
          return (
            <li key={item.href}>
              <Link
                href={item.href}
                aria-current={current ? "page" : undefined}
                className={`block rounded-md px-2.5 py-1.5 text-sm ${
                  current ? "bg-sunken font-medium text-ink" : "text-muted hover:text-ink"
                }`}
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
