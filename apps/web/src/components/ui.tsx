import Link from "next/link";
import type { ButtonHTMLAttributes, ComponentProps, ReactNode } from "react";

/**
 * The few primitives every screen uses. Visual decisions live in the
 * design tokens (globals.css); these only map intent to tokens.
 */
type Variant = "primary" | "secondary" | "quiet" | "danger";

const VARIANTS: Record<Variant, string> = {
  primary: "bg-accent text-accent-ink hover:opacity-90 border border-transparent",
  secondary: "bg-surface text-ink border border-line-strong hover:bg-sunken",
  quiet: "bg-transparent text-muted hover:text-ink border border-transparent underline-offset-4 hover:underline",
  danger: "bg-surface text-negative border border-line-strong hover:bg-negative-soft",
};

const BASE =
  "inline-flex items-center justify-center gap-1.5 rounded-md px-3 py-1.5 text-sm font-medium " +
  "transition-colors disabled:cursor-not-allowed disabled:opacity-50 min-h-9";

export function Button({
  variant = "secondary",
  className = "",
  type = "button",
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant }) {
  return <button type={type} className={`${BASE} ${VARIANTS[variant]} ${className}`} {...props} />;
}

export function LinkButton({
  variant = "secondary",
  className = "",
  ...props
}: ComponentProps<typeof Link> & { variant?: Variant }) {
  return <Link className={`${BASE} ${VARIANTS[variant]} ${className}`} {...props} />;
}

type Tone = "info" | "caution" | "error" | "success";

const TONES: Record<Tone, string> = {
  info: "bg-sunken text-ink border-line",
  caution: "bg-caution-soft text-ink border-line",
  error: "bg-negative-soft text-ink border-line",
  success: "bg-accent-soft text-ink border-line",
};

export function Notice({
  tone = "info",
  title,
  children,
  role,
}: {
  tone?: Tone;
  title?: string;
  children?: ReactNode;
  role?: "status" | "alert";
}) {
  return (
    <div role={role} className={`rounded-lg border px-4 py-3 text-sm ${TONES[tone]}`}>
      {title && <p className="font-semibold">{title}</p>}
      {children && <div className={title ? "mt-1 text-muted" : "text-muted"}>{children}</div>}
    </div>
  );
}

export function PageHeader({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <header className="mb-8">
      <h1 className="font-serif text-3xl leading-tight tracking-tight sm:text-4xl">{title}</h1>
      {children && <div className="mt-2 text-muted">{children}</div>}
    </header>
  );
}

export function Section({
  title,
  description,
  children,
  id,
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  id?: string;
}) {
  const headingId = id ? `${id}-heading` : undefined;
  return (
    <section aria-labelledby={headingId} className="mt-10 first:mt-0">
      <h2 id={headingId} className="font-serif text-xl">
        {title}
      </h2>
      {description && <p className="mt-1 text-sm text-muted">{description}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

export function Card({ children, className = "", as: As = "article" }: { children: ReactNode; className?: string; as?: "article" | "div" | "li" }) {
  return <As className={`rounded-xl border border-line bg-surface p-5 sm:p-6 ${className}`}>{children}</As>;
}
