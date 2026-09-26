import type { Metadata } from "next";

import { createToken, revokeToken, saveNotifications, signOutEverywhere } from "@/app/actions";
import { AssistantTokens } from "@/components/assistant-tokens";
import { NotificationSettings } from "@/components/notification-settings";
import { Button, PageHeader, Section } from "@/components/ui";
import { api, load } from "@/lib/api";
import { config } from "@/lib/config";
import { getSession } from "@/lib/session";

export const metadata: Metadata = { title: "Settings" };

export default async function SettingsPage() {
  const [notifications, tokens, account, session] = await Promise.all([
    load(() => api.notifications()),
    load(() => api.tokens()),
    load(() => api.account()),
    getSession(),
  ]);
  const mcpUrl = `${config().apiPublicUrl}/mcp`;
  return (
    <div>
      <PageHeader title="Settings" />

      <Section title="Email notifications" id="notifications">
        <NotificationSettings initial={notifications} suggestedEmail={session?.email} save={saveNotifications} />
      </Section>

      <Section
        title="Use JobHunt from your AI assistant"
        id="assistant"
        description="Claude, ChatGPT and other MCP clients can use the same account: the same feed, profile and feedback."
      >
        <ol className="list-decimal space-y-2 pl-5 text-sm">
          <li>
            Add a remote MCP server (a “custom connector”) with this URL:
            <code className="mt-1 block break-all rounded bg-sunken px-2 py-1 font-mono text-xs">{mcpUrl}</code>
          </li>
          <li>Sign in when your assistant asks: it uses the same sign-in as this site.</li>
          <li>If your assistant can&apos;t sign in with OAuth, create a token below and give it as a bearer token.</li>
        </ol>
        <p className="mt-3 text-sm text-muted">
          Then ask things like “Find me new jobs”, “Show only small teams”, “Don&apos;t show jobs like this again”, “Why is
          this worth my time?” or “Help me apply to the second one” — it only uses evidence you&apos;ve approved.
        </p>
        <div className="mt-5">
          <AssistantTokens tokens={tokens.tokens} create={createToken} revoke={revokeToken} />
        </div>
      </Section>

      <Section title="Account" id="account">
        <p className="text-sm text-muted">
          Account <code className="font-mono text-xs">{account.id}</code>
          {session?.email && <> · signed in as {session.email}</>}
        </p>
        <div className="mt-4 flex flex-wrap gap-3">
          <form action="/auth/signout" method="post">
            <Button type="submit">Sign out</Button>
          </form>
          <form action={signOutEverywhere}>
            <Button type="submit" variant="quiet">
              Sign out everywhere
            </Button>
          </form>
        </div>
      </Section>
    </div>
  );
}
