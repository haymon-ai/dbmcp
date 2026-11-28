import type { Metadata } from "next";
import { Arvo, IBM_Plex_Mono, IBM_Plex_Sans } from "next/font/google";
import Script from "next/script";
import { Provider } from "@/components/provider";
import { appName, homeTitle, siteUrl } from "@/lib/shared";
import "./global.css";

const plexSans = IBM_Plex_Sans({
  subsets: ["latin"],
  variable: "--font-plex-sans",
});

const plexMono = IBM_Plex_Mono({
  subsets: ["latin"],
  weight: ["400", "500"],
  variable: "--font-plex-mono",
});

const arvo = Arvo({
  subsets: ["latin"],
  weight: "700",
  variable: "--font-arvo",
});

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: homeTitle,
    template: `%s | ${appName}`,
  },
  description:
    "The privacy first MCP server for SQL databases.",
  openGraph: {
    title: homeTitle,
    description:
      "The privacy first MCP server for SQL databases.",
    url: siteUrl,
    siteName: appName,
    type: "website",
  },
  // No title/description here: Twitter then falls back to each page's Open Graph values.
  twitter: {
    card: "summary_large_image",
  },
};

export default function Layout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" className={`${plexSans.variable} ${plexMono.variable} ${arvo.variable}`} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        {/* The router reads a leading `//` as another host and crashes on hydration; a static host cannot redirect. */}
        <Script id="collapse-leading-slashes" strategy="beforeInteractive">
          {`(function(l){var p=l.pathname.replace(/^\\/{2,}/,'/');if(p!==l.pathname)history.replaceState(history.state,'',p+l.search+l.hash)})(location)`}
        </Script>
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
