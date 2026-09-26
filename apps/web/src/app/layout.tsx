import "@fontsource-variable/instrument-sans/wdth.css";
import { GeistMono } from "geist/font/mono";
import type { Metadata, Viewport } from "next";
import { cookies } from "next/headers";
import type { ReactNode } from "react";

import { THEME_COOKIE, parseTheme } from "@/lib/theme";

import "./globals.css";

export const metadata: Metadata = {
  title: { default: "Narrow", template: "%s · Narrow" },
  description: "The few job opportunities worth your time, verified and explained.",
  applicationName: "Narrow",
  robots: { index: false, follow: false },
};

export const viewport: Viewport = {
  themeColor: [
    { media: "(prefers-color-scheme: light)", color: "#f7f7f8" },
    { media: "(prefers-color-scheme: dark)", color: "#0c0d0e" },
  ],
};

export default async function RootLayout({ children }: { children: ReactNode }) {
  const theme = parseTheme((await cookies()).get(THEME_COOKIE)?.value);
  return (
    <html lang="en" className={GeistMono.variable} data-theme={theme === "system" ? undefined : theme}>
      <body className="min-h-dvh bg-ground text-fg">
        <a
          href="#main"
          className="sr-only rounded-md bg-overlay px-3 py-2 text-ui-m shadow-overlay focus:not-sr-only focus:fixed focus:top-4 focus:left-4 focus:z-50"
        >
          Skip to content
        </a>
        {children}
      </body>
    </html>
  );
}
