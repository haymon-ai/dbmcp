import { flattenTree } from "fumadocs-core/page-tree";
import { getLLMText } from "@/lib/llm";
import { source } from "@/lib/source";

export const dynamic = "force-static";

export async function GET() {
  // Sidebar order. Introduction is hidden from the sidebar tree, so it goes first; pages
  // missing from every meta.json follow at the end instead of being dropped.
  const intro = source.getPage([]);
  const tree = flattenTree(source.getPageTree().children).flatMap(
    (item) => source.getNodePage(item) ?? [],
  );
  const ordered = [...(intro ? [intro] : []), ...tree];
  const rest = source.getPages().filter((page) => !ordered.includes(page));
  const texts = await Promise.all([...ordered, ...rest].map(getLLMText));
  return new Response(texts.join("\n\n"));
}
