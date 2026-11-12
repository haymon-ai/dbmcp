import type { MetadataRoute } from "next";
import { basePath, siteUrl } from "@/lib/shared";
import { source } from "@/lib/source";

// Static export only prerenders metadata routes marked static.
export const dynamic = "force-static";

/** Canonical HTML pages only: no /docs/* redirects, no /llms.mdx copies. */
export default function sitemap(): MetadataRoute.Sitemap {
  return source.getPages().map((page) => ({
    url: `${siteUrl}${basePath}${page.url === "/" ? "/" : `${page.url}/`}`,
  }));
}
