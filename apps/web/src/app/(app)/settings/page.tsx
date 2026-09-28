import type { Metadata } from "next";
import { cookies } from "next/headers";

import { createToken, revokeToken, saveNotifications, signOutEverywhere } from "@/app/actions";
import { AssistantTokens } from "@/components/assistant-tokens";
import { NotificationSettings } from "@/components/notification-settings";
import { RowGroup, SummaryRow, SummarySection } from "@/components/summary";
import { ThemeControl } from "@/components/theme-control";
import { Button, PageHeader, SystemStatus, buttonClass } from "@/components/ui";
import { api, load } from "@/lib/api";
import { config } from "@/lib/config";
import { ago } from "@/lib/format";
import { getSession } from "@/lib/session";
import { THEME_COOKIE, parseTheme } from "@/lib/theme";

export const metadata: Metadata = { title: "Settings" };

function issuerLabel(issuer: string): string {
  try {
    return new URL(issuer).host;
  } catch {
    return issuer;
  }
}

/**
 * Everyday settings first (appearance, notifications, account, data);
 * connecting an AI assistant and the service's status under Advanced,
 * each summarized, with the details a labelled action away.
 */
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
  const activeTokens = tokens.tokens.filter((t) => !t.revoked_at && (!t.expires_at || new Date(t.expires_at) > new Date()));
  const lastUsed = activeTokens.map((t) => t.last_used_at).filter((d): d is string => Boolean(d)).sort().at(-1);
  return (
    <div className="max-w-[640px]">
      <PageHeader title="Settings" />

      <div className="flex flex-col gap-11 max-sm:gap-9">
        <RowGroup id="appearance" title="Appearance">
          <SummaryRow id="theme" label="Theme" control={<ThemeControl initial={parseTheme(jar.get(THEME_COOKIE)?.value)} />} />
        </RowGroup>

        <section id="notifications" aria-labelledby="notifications-heading" className="scroll-mt-20">
          <h2 id="notifications-heading" className="mb-1.5 text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
            Notifications
          </h2>
          <NotificationSettings initial={notifications} suggestedEmail={session?.email} save={saveNotifications} />
        </section>

        <RowGroup id="account" title="Account">
          {session?.name && <SummaryRow id="account-name" label="Name" value={session.name} />}
          {session?.email && <SummaryRow id="account-email" label="Email" value={session.email} />}
          <SummaryRow
            id="account-signin"
            label="Sign-in"
            value={account.authenticated_with === "dev" ? "Development sign-in" : identity ? issuerLabel(identity.issuer) : "Your identity provider"}
            importance={identity ? `last signed in ${ago(identity.last_login_at)}` : undefined}
          />
          <div className="flex flex-wrap gap-2 border-t border-line-subtle py-3.5 max-sm:grid max-sm:grid-cols-2">
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
        </RowGroup>

        <RowGroup id="data" title="Your data">
          <SummaryRow
            id="data-export"
            label="Export"
            value="Profile, preferences and every decision"
            action={
              <a href="/api/export" className={buttonClass("secondary", "sm", "max-sm:h-11")}>
                Download my data
              </a>
            }
          />
        </RowGroup>

        <section aria-labelledby="advanced-heading">
          <h2 id="advanced-heading" className="mb-1.5 text-[15px] leading-[1.4] font-semibold tracking-[-0.005em]">
            Advanced
          </h2>
          <div className="border-b border-line-subtle">
            <SummarySection
              id="assistant"
              label="Assistant access"
              conclusion={
                activeTokens.length > 0 ? (
                  <>
                    {activeTokens.length} active {activeTokens.length === 1 ? "token" : "tokens"}
                    {lastUsed && <span className="text-fg-muted"> · last used {ago(lastUsed)}</span>}
                  </>
                ) : (
                  "Use Narrow from Claude, ChatGPT or another MCP client, with this account."
                )
              }
              more="Connection details and tokens"
            >
              <div className="border-b border-line-subtle">
                <SummaryRow id="mcp-url" label="Server URL" value={<code className="font-mono text-mono-s break-all">{mcpUrl}</code>} />
                <SummaryRow id="mcp-signin" label="Sign-in" value="Same as this site. A client that can't sign in can use a token." />
              </div>
              <div className="mt-5">
                <AssistantTokens tokens={tokens.tokens} create={createToken} revoke={revokeToken} />
              </div>
            </SummarySection>
            <SummarySection
              id="status"
              label="System status"
              conclusion={<SystemStatus>Connected</SystemStatus>}
              more="Diagnostics"
            >
              <div className="border-b border-line-subtle">
                <SummaryRow
                  id="status-cli"
                  label="Command line"
                  value={lastSync ? `Synced ${ago(lastSync)}` : "Never synced from the command line"}
                  unset={!lastSync}
                />
                <SummaryRow id="status-account" label="Account ID" value={<code className="font-mono text-mono-s text-fg-secondary">{account.id}</code>} />
              </div>
            </SummarySection>
          </div>
        </section>
      </div>
    </div>
  );
}
