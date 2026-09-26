import type { NextConfig } from "next";

// The web app talks to JobHunt Cloud from the server only (the browser never
// sees an access token), so no API origin appears in the client bundle.
const config: NextConfig = {
  // The container image builds a self-contained server (NEXT_OUTPUT=standalone);
  // `next start` (local, end-to-end tests) needs the default output.
  output: process.env.NEXT_OUTPUT === "standalone" ? "standalone" : undefined,
  poweredByHeader: false,
  reactStrictMode: true,
  typedRoutes: false,
  // No generated AGENTS.md / CLAUDE.md in the app directory.
  agentRules: false,
  // The end-to-end stack serves on 127.0.0.1 (development server only).
  allowedDevOrigins: ["127.0.0.1"],
  async headers() {
    return [
      {
        source: "/:path*",
        headers: [
          { key: "X-Content-Type-Options", value: "nosniff" },
          { key: "Referrer-Policy", value: "strict-origin-when-cross-origin" },
          { key: "X-Frame-Options", value: "DENY" },
          { key: "Permissions-Policy", value: "camera=(), microphone=(), geolocation=()" },
        ],
      },
    ];
  },
};

export default config;
