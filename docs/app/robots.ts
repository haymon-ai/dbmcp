import type { MetadataRoute } from "next";
import { basePath, siteUrl } from "@/lib/shared";

// Static export only prerenders metadata routes marked static.
export const dynamic = "force-static";

/** Allows every crawler, AI crawlers included, and points them at the sitemap. */
export default function robots(): MetadataRoute.Robots {
  return {
    rules: [{ userAgent: "*", allow: "/" }],
    sitemap: `${siteUrl}${basePath}/sitemap.xml`,
  };
}
