import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { generateOGImage } from "fumadocs-ui/og";
import { notFound } from "next/navigation";
import { source } from "@/lib/source";

export const dynamic = "force-static";

// Read once at build time; the wordmark already names the site, so no `site` text.
const wordmark = readFile(join(process.cwd(), "public/logo.svg"), "base64");

/** Link-preview image (1200x630) for one page, at `/og/<slug>/image.png`. */
export async function GET(
  _req: Request,
  { params }: RouteContext<"/og/[...slug]">,
) {
  const { slug } = await params;
  if (slug.at(-1) !== "image.png") notFound();
  const page = source.getPage(slug.slice(0, -1));
  if (!page) notFound();
  return generateOGImage({
    title: page.data.title,
    description: page.data.description,
    icon: (
      // biome-ignore lint/performance/noImgElement: rendered to PNG by next/og, not served as HTML.
      <img
        src={`data:image/svg+xml;base64,${await wordmark}`}
        alt=""
        width={168}
        height={65}
      />
    ),
    primaryColor: "rgba(98,91,248,0.45)",
    primaryTextColor: "#625BF8",
  });
}

export function generateStaticParams() {
  return source
    .getPages()
    .map((page) => ({ slug: [...page.slugs, "image.png"] }));
}
