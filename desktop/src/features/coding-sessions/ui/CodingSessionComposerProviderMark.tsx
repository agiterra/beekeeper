import { Bot, TerminalSquare } from "lucide-react";

import claudeLogoUrl from "@/features/onboarding/assets/harness-logos/claude.png?inline";
import { RUNTIME_MARKS } from "@/features/onboarding/ui/HarnessMarks";
import { PRESET_LOGOS } from "@/features/onboarding/ui/RuntimeIcon";
import { cn } from "@/shared/lib/cn";

/**
 * Which runtime's mark identifies an execution, from the labels it carries.
 *
 * Only marks this app already bundles, with recorded provenance
 * (`desktop/public/harness-logos/CREDITS.md`), are ever drawn: Claude's logo,
 * the inline Cursor and Goose marks, and the preset harness logos the runtime
 * gallery shows. Codex gets the same neutral terminal glyph the gallery gives
 * it: the OpenAI mark was removed from simple-icons at the vendor's request
 * and is deliberately not bundled (CREDITS.md), so the composer must not
 * draw a guess at it. Anything unrecognised gets the generic agent glyph.
 */
export type CodingSessionProviderMarkKind =
  | "claude"
  | "codex"
  | "cursor"
  | "goose"
  | keyof typeof PRESET_LOGOS;

const PRESET_KINDS = Object.keys(PRESET_LOGOS);

/**
 * Pick a mark from the runtime label, then the provider label, then the
 * model id. A token match, not a substring, so a provider instance named
 * after a person ("Claudette") is not mistaken for Claude.
 */
export function codingSessionProviderMarkKind(
  ...labels: readonly (string | null | undefined)[]
): CodingSessionProviderMarkKind | null {
  for (const label of labels) {
    if (!label) continue;
    const tokens = label.toLowerCase().split(/[^a-z0-9]+/);
    if (tokens.includes("claude")) return "claude";
    if (tokens.includes("codex")) return "codex";
    if (tokens.includes("cursor")) return "cursor";
    if (tokens.includes("goose")) return "goose";
    const preset = PRESET_KINDS.find((kind) => tokens.includes(kind));
    if (preset) return preset;
  }
  return null;
}

/** The provider's mark before the model name in the identity chip (SV-18). */
export function CodingSessionComposerProviderMark({
  className,
  kind,
}: {
  className?: string;
  kind: CodingSessionProviderMarkKind | null;
}) {
  const logoUrl =
    kind === "claude" ? claudeLogoUrl : kind ? PRESET_LOGOS[kind] : undefined;
  if (kind && logoUrl) {
    return (
      <img
        alt=""
        aria-hidden
        className={cn(
          "size-3.5 shrink-0 rounded-sm object-contain",
          // The same backing the runtime gallery gives these two marks, so a
          // dark mark stays legible on a dark chip.
          kind === "omp" && "bg-[#0d0d0d]",
          kind === "grok" && "bg-white",
          className,
        )}
        data-provider-mark={kind}
        src={logoUrl}
      />
    );
  }
  const Mark = kind ? RUNTIME_MARKS[kind] : undefined;
  if (kind && Mark) {
    return (
      <span className="inline-flex shrink-0" data-provider-mark={kind}>
        <Mark className={cn("size-3.5", className)} />
      </span>
    );
  }
  if (kind === "codex") {
    return (
      <TerminalSquare
        aria-hidden
        className={cn("size-3.5 shrink-0", className)}
        data-provider-mark="codex"
        strokeWidth={1.5}
      />
    );
  }
  return (
    <Bot
      aria-hidden
      className={cn("size-3 shrink-0", className)}
      data-provider-mark="generic"
    />
  );
}
