import type { InferPageType } from "fumadocs-core/source";
import { basePath, siteUrl } from "./shared";
import type { source } from "./source";

type Page = InferPageType<typeof source>;

/** Markdown copy of a page, served under `/llms.mdx/<slug>/content.md`. */
export function markdownUrl(page: Page): string {
  return `${basePath}/llms.mdx/${[...page.slugs, "content.md"].join("/")}`;
}

/** Link-preview image of a page, served under `/og/<slug>/image.png`. */
export function ogImageUrl(page: Page): string {
  return `${basePath}/og/${[...page.slugs, "image.png"].join("/")}`;
}

/** Absolute URL of a page's HTML version, with the trailing slash the site uses. */
export function pageUrl(page: Page): string {
  return `${siteUrl}${basePath}${page.url === "/" ? "/" : `${page.url}/`}`;
}

/** Reads `name="value"` attributes from a JSX tag's attribute text. */
function attrs(text: string): Record<string, string> {
  return Object.fromEntries(
    [...text.matchAll(/(\w+)="([^"]*)"/g)].map(([, k, v]) => [k, v]),
  );
}

/** Replaces the inline `<Dbmcp />` wordmark component with plain text. */
export function plainWordmark(text: string): string {
  return text.replace(/<Dbmcp\s*\/>/g, "dbmcp");
}

/** Replaces the custom MDX components with plain Markdown. */
function plainMarkdown(body: string): string {
  return (
    plainWordmark(body)
      // Demo recordings become images; defaults match components/demo.tsx.
      .replace(/<Demo\b([^>]*)\/>/g, (_, text: string) => {
        const {
          src = "/demo.gif",
          alt = "Demo showing natural-language database queries answered instantly",
        } = attrs(text);
        return `![${alt}](${siteUrl}${basePath}${src})`;
      })
      // The grid only lays out its list.
      .replace(/^<\/?DatabaseGrid>\n\n?/gm, "")
      // Callouts become blockquotes headed by their title, or "Warning" / "Note".
      .replace(
        /<Callout\b([^>]*)>\n?([\s\S]*?)\n?<\/Callout>/g,
        (_, text: string, inner: string) => {
          const { title, type } = attrs(text);
          const label =
            title ??
            (type === "warn" || type === "warning" || type === "error"
              ? "Warning"
              : "Note");
          return `> **${label}:** ${inner.trim().split("\n").join("\n> ")}`;
        },
      )
  );
}

/** A page rendered as plain Markdown for LLMs, headed by its title and source URL. */
export async function getLLMText(page: Page): Promise<string> {
  const raw = await page.data.getText("raw");
  const body = plainMarkdown(raw.replace(/^---\n[\s\S]*?\n---\n/, "")).trim();
  const description = page.data.description
    ? `${page.data.description}\n\n`
    : "";
  return `# ${page.data.title}\n\nSource: ${pageUrl(page)}\n\n${description}${body}`;
}
