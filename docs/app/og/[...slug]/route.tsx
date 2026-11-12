import { readFile } from "node:fs/promises";
import { join } from "node:path";
import { ImageResponse } from "next/og";
import { notFound } from "next/navigation";
import { source } from "@/lib/source";

export const dynamic = "force-static";

// Read once at build time.
const wordmark = readFile(join(process.cwd(), "public/logo.svg"), "base64");

// IBM Plex Sans, as on the site. next/og needs TTF/OTF/WOFF (not WOFF2), so the files are
// vendored from github.com/IBM/plex (SIL OFL 1.1, see assets/fonts/OFL.txt).
const font = (file: string) =>
  readFile(join(process.cwd(), "assets/fonts", file));
const regular = font("IBMPlexSans-Regular.ttf");
const bold = font("IBMPlexSans-Bold.ttf");

// Site colors from app/global.css (light theme).
const foreground = "#161616";
const muted = "#525252";
const primary = "#625BF8";

/** Link-preview image (1200x630) for one page, at `/og/<slug>/image.png`. */
export async function GET(
  _req: Request,
  { params }: RouteContext<"/og/[...slug]">,
) {
  const { slug } = await params;
  if (slug.at(-1) !== "image.png") notFound();
  const page = source.getPage(slug.slice(0, -1));
  if (!page) notFound();
  return new ImageResponse(
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        width: "100%",
        height: "100%",
        padding: "72px 80px",
        backgroundColor: "#ffffff",
        borderBottom: `16px solid ${primary}`,
        fontFamily: "IBM Plex Sans",
      }}
    >
      {/* biome-ignore lint/performance/noImgElement: rendered to PNG by next/og, not served as HTML. */}
      <img
        src={`data:image/svg+xml;base64,${await wordmark}`}
        alt=""
        width={155}
        height={60}
      />
      <div
        style={{ display: "flex", flexDirection: "column", marginTop: "auto" }}
      >
        <div
          style={{
            fontSize: 76,
            fontWeight: 700,
            lineHeight: 1.1,
            color: foreground,
          }}
        >
          {page.data.title}
        </div>
        {page.data.description && (
          <div
            style={{
              marginTop: 24,
              fontSize: 36,
              lineHeight: 1.35,
              color: muted,
            }}
          >
            {page.data.description}
          </div>
        )}
      </div>
    </div>,
    {
      width: 1200,
      height: 630,
      fonts: [
        { name: "IBM Plex Sans", data: await regular, weight: 400 },
        { name: "IBM Plex Sans", data: await bold, weight: 700 },
      ],
    },
  );
}

export function generateStaticParams() {
  return source
    .getPages()
    .map((page) => ({ slug: [...page.slugs, "image.png"] }));
}
