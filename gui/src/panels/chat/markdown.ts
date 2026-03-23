/**
 * Lightweight markdown-to-HTML renderer for the chat panel.
 *
 * Supports: fenced code blocks (```), inline code (`), bold (**),
 * italic (*), headers (#-###), links ([text](url)), and unordered lists (- item).
 *
 * Security: HTML entities are escaped BEFORE markdown transforms to prevent XSS.
 */

/** Escape HTML entities to prevent XSS. */
function escapeHtml(text: string): string {
  return text
    .replace(/&/g, '&amp;')
    .replace(/</g, '&lt;')
    .replace(/>/g, '&gt;')
    .replace(/"/g, '&quot;')
    .replace(/'/g, '&#39;');
}

/**
 * Check whether a URL has a safe protocol for use in href attributes.
 * Only http://, https://, and mailto: are allowed.
 * URLs have already been HTML-entity-escaped at this point, but these
 * protocols contain no HTML-special characters so startsWith works correctly.
 */
function isSafeUrl(url: string): boolean {
  const lower = url.toLowerCase();
  return (
    lower.startsWith('http://') ||
    lower.startsWith('https://') ||
    lower.startsWith('mailto:')
  );
}

/**
 * Render a markdown string to sanitized HTML.
 *
 * The input is first HTML-escaped, then markdown transformations are applied
 * sequentially. This approach is safe because all user/LLM content has its
 * HTML entities neutralized before any HTML tags are introduced by the renderer.
 */
export function renderMarkdown(input: string): string {
  if (!input) return '';

  // Phase 1: Escape HTML entities in the raw input
  let html = escapeHtml(input);

  // Phase 2: Extract fenced code blocks before inline transforms
  // Replace fenced code blocks (``` ... ```) with placeholders
  const codeBlocks: string[] = [];
  html = html.replace(/```(\w*)\n([\s\S]*?)```/g, (_match, lang, code) => {
    const idx = codeBlocks.length;
    const langAttr = lang ? ` class="language-${lang}"` : '';
    codeBlocks.push(`<pre><code${langAttr}>${code.replace(/\n$/, '')}</code></pre>`);
    return `\x00CODEBLOCK${idx}\x00`;
  });

  // Phase 3: Process line-by-line for headers and lists
  const lines = html.split('\n');
  const processed: string[] = [];
  let inList = false;

  for (const line of lines) {
    // Skip code block placeholders
    if (line.match(/\x00CODEBLOCK\d+\x00/)) {
      if (inList) {
        processed.push('</ul>');
        inList = false;
      }
      processed.push(line);
      continue;
    }

    // Headers
    const headerMatch = line.match(/^(#{1,3})\s+(.+)$/);
    if (headerMatch) {
      if (inList) {
        processed.push('</ul>');
        inList = false;
      }
      const level = headerMatch[1].length;
      processed.push(`<h${level}>${headerMatch[2]}</h${level}>`);
      continue;
    }

    // Unordered list items
    const listMatch = line.match(/^-\s+(.+)$/);
    if (listMatch) {
      if (!inList) {
        processed.push('<ul>');
        inList = true;
      }
      processed.push(`<li>${listMatch[1]}</li>`);
      continue;
    }

    // Close list if we're in one and hit a non-list line
    if (inList) {
      processed.push('</ul>');
      inList = false;
    }

    processed.push(line);
  }

  if (inList) {
    processed.push('</ul>');
  }

  html = processed.join('\n');

  // Phase 4: Inline transforms (order matters)
  // Inline code (must come before bold/italic to prevent conflicts)
  html = html.replace(/`([^`]+)`/g, '<code>$1</code>');

  // Bold
  html = html.replace(/\*\*([^*]+)\*\*/g, '<strong>$1</strong>');

  // Italic (single asterisk, but not inside bold)
  html = html.replace(/(?<!\*)\*([^*]+)\*(?!\*)/g, '<em>$1</em>');

  // Links [text](url) — with protocol allowlist to prevent javascript:/data:/vbscript: XSS
  html = html.replace(
    /\[([^\]]+)\]\(([^)]+)\)/g,
    (_match: string, text: string, url: string) => {
      if (isSafeUrl(url)) {
        return `<a href="${url}" target="_blank" rel="noopener noreferrer">${text}</a>`;
      }
      // Unsafe protocol — render link text as plain text without hyperlink
      return text;
    },
  );

  // Phase 5: Restore code blocks
  html = html.replace(/\x00CODEBLOCK(\d+)\x00/g, (_match, idx) => {
    return codeBlocks[parseInt(idx, 10)];
  });

  // Phase 6: Convert remaining newlines to <br> (but not inside pre/code blocks)
  // Simple approach: only convert \n that are not adjacent to block elements
  html = html.replace(/\n(?!<\/?(?:pre|code|ul|li|h[1-6]))/g, '<br>\n');

  return html;
}
