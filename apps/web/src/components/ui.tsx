import Link from "next/link";
import type { ButtonHTMLAttributes, ComponentProps, ReactNode } from "react";

/**
 * The primitives every screen uses. Visual decisions live in the design
 * tokens (globals.css); these only map intent to tokens. There is no
 * generic card: hierarchy comes from type, ink, spacing and hairlines, and
 * the one raised surface on a screen is `Raised`.
 */
type Variant = "primary" | "secondary" | "ghost" | "destructive" | "danger";
type Size = "sm" | "md" | "lg";

const BUTTON_BASE =
  "inline-flex items-center justify-center gap-2 rounded-md border text-ui-m leading-none " +
  "whitespace-nowrap cursor-pointer select-none transition-colors duration-[120ms] ease-out active:duration-[80ms] " +
  "disabled:cursor-default disabled:border-line-subtle disabled:bg-transparent disabled:text-fg-disabled";

const VARIANTS: Record<Variant, string> = {
  // Ink, not accent: the accent is information, never a big filled CTA.
  primary: "border-transparent bg-action font-semibold text-fg-inverse hover:bg-action-hover active:bg-action-pressed",
  secondary: "border-line text-fg-body hover:border-line-strong hover:text-fg active:bg-ground-hover",
  ghost: "border-transparent text-fg-secondary hover:text-fg active:bg-ground-hover",
  destructive: "border-line text-danger hover:border-line-strong active:bg-ground-hover",
  danger: "border-transparent bg-danger font-semibold text-fg-inverse hover:opacity-90",
};

const SIZES: Record<Size, string> = {
  sm: "h-7 px-3 text-[12px]",
  md: "h-8 px-3",
  lg: "h-10 px-[18px] text-[14px]",
};

// Primary and danger carry a little more horizontal room.
const PADDED: Partial<Record<Variant, Record<Size, string>>> = {
  primary: { sm: "px-3", md: "px-4", lg: "px-5" },
  danger: { sm: "px-3", md: "px-4", lg: "px-5" },
};

export function buttonClass(variant: Variant = "secondary", size: Size = "md", className = ""): string {
  return [BUTTON_BASE, VARIANTS[variant], SIZES[size], PADDED[variant]?.[size] ?? "", className].join(" ");
}

export function Button({
  variant = "secondary",
  size = "md",
  loading = false,
  className = "",
  type = "button",
  children,
  ...props
}: ButtonHTMLAttributes<HTMLButtonElement> & { variant?: Variant; size?: Size; loading?: boolean }) {
  return (
    <button type={type} className={buttonClass(variant, size, className)} aria-busy={loading || undefined} {...props}>
      {children}
      {loading && <Spinner />}
    </button>
  );
}

export function LinkButton({
  variant = "secondary",
  size = "md",
  className = "",
  ...props
}: ComponentProps<typeof Link> & { variant?: Variant; size?: Size }) {
  return <Link className={buttonClass(variant, size, className)} {...props} />;
}

/** A quiet text action inside a row ("Edit", "Remove"). */
export const inlineActionClass =
  "cursor-pointer font-sans text-[12.5px] font-medium text-fg-secondary transition-colors duration-[120ms] hover:text-fg disabled:cursor-default disabled:text-fg-disabled";

/** A link in running text: the accent is how links read. */
export const textLinkClass = "font-medium text-accent transition-colors duration-[120ms] hover:text-accent-hover";

/** Loading keeps the label and adds a 10px spinner. */
export function Spinner({ label }: { label?: string }) {
  return (
    <span
      role={label ? "status" : undefined}
      aria-label={label}
      aria-hidden={label ? undefined : true}
      className="inline-block size-2.5 shrink-0 animate-[nr-spin_0.8s_linear_infinite] rounded-full border-[1.5px] border-current/25 border-t-current"
    />
  );
}

/* Form controls. Mobile controls are 44px tall with 16px text (no zoom). */
export const inputClass =
  "h-9 w-full min-w-0 rounded-md border border-line bg-inset px-3 text-[13.5px] text-fg nr-tnum placeholder:text-fg-muted " +
  "transition-colors duration-[120ms] hover:border-line-strong focus-visible:border-line-strong " +
  "aria-[invalid=true]:border-danger/40 disabled:border-line-subtle disabled:bg-ground disabled:text-fg-disabled " +
  "max-sm:h-11 max-sm:text-[16px]";

export const selectClass = `${inputClass} nr-select cursor-pointer appearance-none pr-8`;

export const textareaClass =
  "w-full min-w-0 rounded-md border border-line bg-inset px-3 py-2.5 text-[14px] leading-normal text-fg placeholder:text-fg-muted " +
  "transition-colors duration-[120ms] hover:border-line-strong focus-visible:border-line-strong max-sm:text-[16px]";

export const labelClass = "mb-1.5 block text-label text-fg-secondary";
export const helpClass = "mt-1.5 text-caption text-fg-muted";

type Tone = "info" | "caution" | "error" | "success";

const TONE_MARKER: Record<Tone, string> = {
  info: "bg-info",
  caution: "bg-warning",
  error: "bg-danger",
  success: "bg-success",
};

/**
 * A system notice: a small square marker, a plain title, a sentence. The
 * title carries the meaning; the marker only reinforces it.
 */
export function Notice({
  tone = "info",
  title,
  children,
  role,
}: {
  tone?: Tone;
  title?: ReactNode;
  children?: ReactNode;
  role?: "status" | "alert";
}) {
  return (
    <div role={role} className="flex gap-2.5 text-body-s">
      <span aria-hidden="true" className={`mt-[7px] size-1.5 shrink-0 rounded-[1px] ${TONE_MARKER[tone]}`} />
      <div className="min-w-0">
        {title && <p className="font-medium text-fg">{title}</p>}
        {children && <div className={title ? "mt-0.5 text-fg-secondary" : "text-fg-secondary"}>{children}</div>}
      </div>
    </div>
  );
}

export function PageHeader({
  title,
  count,
  aside,
  children,
}: {
  title: string;
  count?: ReactNode;
  aside?: ReactNode;
  children?: ReactNode;
}) {
  return (
    <header className="mb-8 max-sm:mb-6">
      <div className="flex flex-wrap items-baseline justify-between gap-x-4 gap-y-1">
        <h1 className="flex items-baseline gap-3 text-heading-l max-sm:text-[26px]">
          {title}
          {count !== undefined && <span className="font-mono text-[13px] font-normal tracking-normal text-fg-muted">{count}</span>}
        </h1>
        {aside}
      </div>
      {children && <div className="mt-2 max-w-[68ch] text-body-s text-pretty text-fg-secondary">{children}</div>}
    </header>
  );
}

export function Section({
  title,
  description,
  children,
  id,
  className = "",
}: {
  title: string;
  description?: ReactNode;
  children: ReactNode;
  id?: string;
  className?: string;
}) {
  const headingId = id ? `${id}-heading` : undefined;
  return (
    <section id={id} aria-labelledby={headingId} className={`mt-11 scroll-mt-20 first:mt-0 max-sm:mt-8 ${className}`}>
      <h2 id={headingId} className="text-heading-m max-sm:text-[18px]">
        {title}
      </h2>
      {description && <p className="mt-1 max-w-[68ch] text-[13px] leading-normal text-fg-muted">{description}</p>}
      <div className="mt-3">{children}</div>
    </section>
  );
}

/** A small section label ("Why it may be worth your time"). */
export function Label({ children, as: As = "p", className = "", id }: { children: ReactNode; as?: "p" | "h2" | "h3" | "dt"; className?: string; id?: string }) {
  return (
    <As id={id} className={`text-label text-fg-muted ${className}`}>
      {children}
    </As>
  );
}

/**
 * The one raised surface on a screen: Today's lead recommendation and the
 * decision brief. Everything else sits on the ground.
 */
export function Raised({
  children,
  className = "",
  as: As = "div",
  ...rest
}: { children: ReactNode; className?: string; as?: "div" | "article" | "section" } & Record<`aria-${string}`, string | undefined>) {
  return (
    <As className={`min-w-0 rounded-lg border border-line-raised bg-raised shadow-raised ${className}`} {...rest}>
      {children}
    </As>
  );
}

/** A row of labelled facts: a grid on wide screens, stacked on phones. */
export function FactRows({ rows, labelWidth = "sm:grid-cols-[150px_minmax(0,1fr)]" }: { rows: { k: ReactNode; v: ReactNode; key?: string }[]; labelWidth?: string }) {
  return (
    <dl className="border-t border-line-subtle">
      {rows.map((r, i) => (
        <div key={r.key ?? i} className={`grid gap-x-4 gap-y-0.5 border-b border-line-subtle py-2.5 text-row ${labelWidth}`}>
          <dt className="text-fg-muted">{r.k}</dt>
          <dd className="min-w-0 text-fg">{r.v}</dd>
        </div>
      ))}
    </dl>
  );
}

/**
 * A system fact in mono ("Checked 20 min ago · next check in about 30 min").
 * The dot is the one round thing in the product: mint when the service is
 * working, blue-grey when it can't be reached.
 */
export function SystemStatus({ children, down = false }: { children: ReactNode; down?: boolean }) {
  return (
    <span className="inline-flex items-center gap-2 font-mono text-mono-s text-fg-muted">
      <span aria-hidden="true" className={`size-[5px] shrink-0 rounded-full ${down ? "bg-info" : "bg-accent"}`} />
      {children}
    </span>
  );
}

type Status = "active" | "waiting" | "offer" | "needs" | "closed";

const STATUS_MARKER: Record<Status, string> = {
  active: "bg-fg",
  waiting: "border border-fg-secondary",
  offer: "bg-success",
  needs: "bg-warning",
  closed: "bg-fg-disabled",
};

const STATUS_INK: Record<Status, string> = {
  active: "text-fg",
  waiting: "text-fg-secondary",
  offer: "text-fg",
  needs: "text-fg",
  closed: "text-fg-muted",
};

/** An application stage: a small square and the stage's word. */
export function StatusText({ status, children, className = "" }: { status: Status; children: ReactNode; className?: string }) {
  return (
    <span className={`inline-flex items-center gap-[9px] text-[13px] ${STATUS_INK[status]} ${className}`}>
      <span aria-hidden="true" className={`size-1.5 shrink-0 rounded-[1px] ${STATUS_MARKER[status]}`} />
      {children}
    </span>
  );
}

/** Restrained loading: the shape of what is coming, softly pulsing. */
export function Skeleton({ variant = "card" }: { variant?: "card" | "row" | "peer" }) {
  const bar = (width: string, height: string, tone: "strong" | "soft", extra = "") => (
    <div className={`${width} ${height} rounded-[2px] ${tone === "strong" ? "bg-overlay-hover" : "bg-selected"} ${extra}`} />
  );
  if (variant === "peer") {
    return (
      <div aria-hidden="true" className="nr-skeleton border-t border-line-subtle pt-4 pb-6">
        {bar("w-[40%]", "h-2", "soft")}
        {bar("w-[80%]", "h-3", "strong", "mt-3.5")}
        {bar("w-[55%]", "h-2", "soft", "mt-4")}
        {bar("w-[45%]", "h-2", "soft", "mt-2")}
        {bar("w-[90%]", "h-2", "soft", "mt-4")}
      </div>
    );
  }
  if (variant === "row") {
    return (
      <div aria-hidden="true" className="nr-skeleton flex h-12 items-center gap-4 border-b border-line-subtle px-3">
        {bar("w-[34%]", "h-2", "strong")}
        {bar("w-[18%]", "h-2", "soft")}
      </div>
    );
  }
  return (
    <div aria-hidden="true" className="nr-skeleton rounded-lg border border-line-raised bg-raised p-[18px]">
      {bar("w-[30%]", "h-2", "strong")}
      {bar("w-[65%]", "h-3.5", "strong", "mt-3.5")}
      {bar("w-[88%]", "h-2", "soft", "mt-[18px]")}
      {bar("w-[72%]", "h-2", "soft", "mt-2")}
    </div>
  );
}
