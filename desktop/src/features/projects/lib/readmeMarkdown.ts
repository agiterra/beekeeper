/**
 * Best-effort normalization of the HTML islands GitHub-flavored READMEs
 * commonly carry (centered headers, badge rows, <br/> spacing) into plain
 * markdown, since the app's markdown renderer deliberately does not render
 * raw HTML. Fenced code and inline code spans pass through untouched.
 */

function decodeHtmlEntities(value: string) {
  return value
    .replace(/&amp;/g, "&")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&#39;/g, "'");
}

function htmlInlineToMarkdown(value: string): string {
  return decodeHtmlEntities(value)
    .replace(/<br\s*\/?\s*>/gi, "\n")
    .replace(/<img\b([^>]*)>/gi, (_match: string, attrs: string) => {
      const src = attrs.match(/\bsrc=["']([^"']+)["']/i)?.[1];
      const alt = attrs.match(/\balt=["']([^"']*)["']/i)?.[1] ?? "";
      return src ? `![${alt}](${src})` : "";
    })
    .replace(
      /<a\b[^>]*\bhref=["']([^"']+)["'][^>]*>([\s\S]*?)<\/a>/gi,
      (_match: string, href: string, label: string) =>
        `[${htmlInlineToMarkdown(label).trim()}](${href})`,
    )
    .replace(/<(strong|b)\b[^>]*>([\s\S]*?)<\/\1>/gi, "**$2**")
    .replace(/<(em|i)\b[^>]*>([\s\S]*?)<\/\1>/gi, "*$2*")
    .replace(/<code\b[^>]*>([\s\S]*?)<\/code>/gi, "`$1`")
    .replace(/<sub\b[^>]*>([\s\S]*?)<\/sub>/gi, "$1")
    .replace(/<span\b[^>]*>([\s\S]*?)<\/span>/gi, "$1")
    .replace(/<[^>]+>/g, "")
    .trim();
}

/**
 * Inline HTML sitting directly at the top level (no block wrapper). Unlike
 * [`htmlInlineToMarkdown`] this neither decodes entities nor strips unknown
 * tags — markdown autolinks (`<https://…>`) and escaped examples must
 * survive. An anchor wrapping a heading hoists the heading out so
 * `<a href="/"># Title</a>` becomes `# [Title](/)`.
 */
function convertTopLevelInlineHtml(value: string): string {
  return value
    .replace(/<br\s*\/?\s*>/gi, "\n")
    .replace(/<img\b([^>]*)\/?>/gi, (_match: string, attrs: string) => {
      const src = attrs.match(/\bsrc=["']([^"']+)["']/i)?.[1];
      const alt = attrs.match(/\balt=["']([^"']*)["']/i)?.[1] ?? "";
      return src ? `![${alt}](${src})` : "";
    })
    .replace(
      /<a\b[^>]*\bhref=["']([^"']+)["'][^>]*>([\s\S]*?)<\/a>/gi,
      (_match: string, href: string, label: string) => {
        const inner = htmlInlineToMarkdown(label).trim();
        const heading = inner.match(/^(#{1,6})\s+([\s\S]*)$/);
        return heading
          ? `\n\n${heading[1]} [${heading[2].trim()}](${href})\n\n`
          : `[${inner}](${href})`;
      },
    )
    .replace(/<(strong|b)\b[^>]*>([\s\S]*?)<\/\1>/gi, "**$2**")
    .replace(/<(em|i)\b[^>]*>([\s\S]*?)<\/\1>/gi, "*$2*")
    .replace(/<sub\b[^>]*>([\s\S]*?)<\/sub>/gi, "$1")
    .replace(/<span\b[^>]*>([\s\S]*?)<\/span>/gi, "$1");
}

function normalizeHtmlSegment(value: string): string {
  return convertTopLevelInlineHtml(
    value
      .replace(
        /<h([1-6])\b[^>]*>([\s\S]*?)<\/h\1>/gi,
        (_match, depth: string, heading: string) =>
          `${"#".repeat(Number(depth))} ${htmlInlineToMarkdown(heading)}\n\n`,
      )
      .replace(
        /<p\b[^>]*>([\s\S]*?)<\/p>/gi,
        (_match, inner: string) => `${htmlInlineToMarkdown(inner)}\n\n`,
      )
      .replace(
        /<div\b[^>]*>([\s\S]*?)<\/div>/gi,
        (_match, inner: string) => `${htmlInlineToMarkdown(inner)}\n\n`,
      )
      .replace(
        /<center\b[^>]*>([\s\S]*?)<\/center>/gi,
        (_match, inner: string) => `${htmlInlineToMarkdown(inner)}\n\n`,
      ),
  );
}

/** Fenced blocks and inline code spans, kept verbatim by the normalizer. */
const CODE_SEGMENT_RE =
  /(^|\n)(```[\s\S]*?(?:\n```|$)|~~~[\s\S]*?(?:\n~~~|$))|(`[^`\n]+`)/g;

export function normalizeReadmeMarkdown(content: string) {
  // Split into code and non-code segments so HTML samples inside fences and
  // inline code stay untouched.
  const segments: { code: boolean; text: string }[] = [];
  let cursor = 0;
  for (const match of content.matchAll(CODE_SEGMENT_RE)) {
    const start = (match.index ?? 0) + (match[1]?.length ?? 0);
    const code = match[2] ?? match[3] ?? "";
    if (start > cursor) {
      segments.push({ code: false, text: content.slice(cursor, start) });
    }
    segments.push({ code: true, text: code });
    cursor = start + code.length;
  }
  if (cursor < content.length) {
    segments.push({ code: false, text: content.slice(cursor) });
  }

  return segments
    .map((segment) =>
      segment.code ? segment.text : normalizeHtmlSegment(segment.text),
    )
    .join("")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}
