import { llms } from "fumadocs-core/source";
import { markdownUrl } from "@/lib/llm";
import { appName, siteUrl } from "@/lib/shared";
import { source } from "@/lib/source";

export const dynamic = "force-static";

// Follows https://llmstxt.org/: an H1, a blockquote summary, then H2 sections of links.
// Links point at each page's Markdown copy, absolute so they work outside the site.
export async function GET() {
  const md = (page: Parameters<typeof markdownUrl>[0]) =>
    `${siteUrl}${markdownUrl(page)}`;
  const index = await llms(source, {
    renderUrl: (node) => {
      const page = source.getNodePage(node);
      return page ? md(page) : `${siteUrl}${node.url}`;
    },
  }).index();
  const sections = index
    .replace(/^# .*\n\n/, "")
    // Sidebar separators become H2 sections; "About" (the license) is the skippable part.
    .replace(/^- \*\*About\*\*$/m, "## Optional\n")
    .replace(/^- \*\*(.+)\*\*$/gm, "## $1\n");

  // The page tree hides Introduction from the sidebar; it heads the list here.
  const intro = source.getPage([]);
  const head = [
    `# ${appName}`,
    `> ${intro?.data.description ?? ""}. Gives AI assistants read-only access to SQL databases over the Model Context Protocol.`,
    `All pages in one file: ${siteUrl}/llms-full.txt`,
    "## Introduction",
    intro
      ? `- [Introduction](${md(intro)}): Overview, quick install, and supported databases`
      : "",
  ].join("\n\n");
  return new Response(`${head}\n${sections}\n`, {
    headers: { "Content-Type": "text/plain; charset=utf-8" },
  });
}
