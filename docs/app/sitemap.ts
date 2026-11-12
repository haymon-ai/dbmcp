import type { MetadataRoute } from "next";
import { pageUrl } from "@/lib/llm";
import { source } from "@/lib/source";

// Static export only prerenders metadata routes marked static.
export const dynamic = "force-static";

/** Canonical HTML pages only: no /docs/* redirects, no /llms.mdx copies. */
export default function sitemap(): MetadataRoute.Sitemap {
  return source.getPages().map((page) => ({ url: pageUrl(page) }));
}
