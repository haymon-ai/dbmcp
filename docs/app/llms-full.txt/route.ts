import { flattenTree } from "fumadocs-core/page-tree";
import { getLLMText } from "@/lib/llm";
import { source } from "@/lib/source";

export const dynamic = "force-static";

export async function GET() {
  // Sidebar order. Introduction is hidden from the sidebar tree, so it goes first.
  const intro = source.getPage([]);
  const pages = flattenTree(source.getPageTree().children).flatMap(
    (item) => source.getNodePage(item) ?? [],
  );
  const texts = await Promise.all(
    (intro ? [intro, ...pages] : pages).map(getLLMText),
  );
  return new Response(texts.join("\n\n"));
}
