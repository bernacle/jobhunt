import type { Metadata } from "next";
import { cookies } from "next/headers";
import type { ReactNode } from "react";

import { createToken, revokeToken, saveNotifications, signOutEverywhere } from "@/app/actions";
import { AssistantTokens } from "@/components/assistant-tokens";
import { NotificationSettings } from "@/components/notification-settings";
import { ThemeControl } from "@/components/theme-control";
import { Button, PageHeader, Section, SystemStatus, buttonClass } from "@/components/ui";
import { api, load } from "@/lib/api";
import { config } from "@/lib/config";
import { ago } from "@/lib/format";
import { getSession } from "@/lib/session";
import { THEME_COOKIE, parseTheme } from "@/lib/theme";

export const metadata: Metadata = { title: "Settings" };

function Row({ k, children, action }: { k: string; children: ReactNode; action?: ReactNode }) {
  return (
    <div className="grid items-baseline gap-x-4 gap-y-0.5 border-t border-line-subtle py-3 text-[14px] leading-normal sm:grid-cols-[150px_minmax(0,1fr)_auto] max-sm:min-h-14">
      <dt className="text-[13px] text-fg-muted max-sm:text-[12.5px]">{k}</dt>
      <dd className="min-w-0 break-words">{children}</dd>
      {action && <dd>{action}</dd>}
    </div>
  );
}

function issuerLabel(issuer: string): string {
  try {
    return new URL(issuer).host;
  } catch {
    return issuer;
  }
}

export default async function SettingsPage() {
  const [notifications, tokens, account, session, jar] = await Promise.all([
    load(() => api.notifications()),
    load(() => api.tokens()),
    load(() => api.account()),
    getSession(),
    cookies(),
  ]);
  const mcpUrl = `${config().apiPublicUrl}/mcp`;
  const identity = account.identities[0];
  const lastSync = account.cloud.last_sync_at;
  return (
    <div className="max-w-[560px]">
      <PageHeader title="Settings" />

      <Section title="Account" id="account">
        <dl>
          {session?.name && <Row k="Name">{session.name}</Row>}
          {session?.email && <Row k="Email">{session.email}</Row>}
          <Row k="Sign-in">
            {account.authenticated_with === "dev" ? "Development sign-in" : identity ? issuerLabel(identity.issuer) : "Your identity provider"}
            {identity && <span className="block font-mono text-mono-s text-fg-muted">last signed in {ago(identity.last_login_at)}</span>}
          </Row>
          <Row k="Account ID">
            <code className="font-mono text-mono-s text-fg-secondary">{account.id}</code>
          </Row>
        </dl>
        <div className="mt-4 flex flex-wrap gap-2 border-t border-line-subtle pt-4 max-sm:grid max-sm:grid-cols-2">
          <form action="/auth/signout" method="post">
            <Button type="submit" className="max-sm:h-11 max-sm:w-full">
              Sign out
            </Button>
          </form>
          <form action={signOutEverywhere}>
            <Button type="submit" variant="ghost" className="max-sm:h-11 max-sm:w-full">
              Sign out everywhere
            </Button>
          </form>
        </div>
      </Section>

      <Section title="Appearance" id="appearance">
        <dl>
          <Row k="Theme">
            <ThemeControl initial={parseTheme(jar.get(THEME_COOKIE)?.value)} />
          </Row>
        </dl>
      </Section>

      <Section title="Notifications" id="notifications">
        <NotificationSettings initial={notifications} suggestedEmail={session?.email} save={saveNotifications} />
      </Section>

      <Section title="Status" id="status">
        <dl>
          <Row k="Narrow">
            <SystemStatus>Connected</SystemStatus>
          </Row>
          <Row k="Command line">
            {lastSync ? (
              <>
                Synced <span className="font-mono text-mono-s text-fg-muted">{ago(lastSync)}</span>
              </>
            ) : (
              <span className="text-fg-muted">Never synced from the command line</span>
            )}
          </Row>
        </dl>
      </Section>

      <Section title="MCP" id="assistant" description="Use Narrow from Claude, ChatGPT or another MCP client, with this same account: the same Today, profile and feedback.">
        <dl>
          <Row k="Server URL">
            <code className="font-mono text-mono-s break-all text-fg">{mcpUrl}</code>
          </Row>
          <Row k="Sign-in">
            Your MCP client asks you to sign in, with the same sign-in as this site.
            <span className="block text-[13px] text-fg-muted">If it can&apos;t sign in with OAuth, give it a token from below as a bearer token.</span>
          </Row>
        </dl>
        <p className="mt-3 border-t border-line-subtle pt-3 text-[13px] leading-normal text-fg-secondary">
          Add a remote MCP server (a “custom connector”) with the URL above, then ask things like “What&apos;s new today?”, “Why is this worth my
          time?” or “Don&apos;t show jobs like this again”.
        </p>
        <div className="mt-5">
          <AssistantTokens tokens={tokens.tokens} create={createToken} revoke={revokeToken} />
        </div>
      </Section>

      <Section title="Your data" id="data">
        <p className="mb-3.5 text-body-s text-fg-secondary">
          Download everything Narrow keeps about you as a portable file: your profile, preferences and every decision it learned from.
        </p>
        <a href="/api/export" className={buttonClass("secondary", "md", "max-sm:h-11 max-sm:w-full")}>
          Download my data
        </a>
      </Section>
    </div>
  );
}
