import type { Metadata } from "next";
import { IBM_Plex_Mono, IBM_Plex_Sans } from "next/font/google";
import { Provider } from "@/components/provider";
import { siteUrl } from "@/lib/shared";
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

export const metadata: Metadata = {
  metadataBase: new URL(siteUrl),
  title: {
    default: "Haymon Database MCP — AI access to your SQL databases",
    template: "%s | Haymon Database MCP",
  },
  description:
    "A single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite. Zero runtime dependencies, read-only by default.",
  openGraph: {
    title: "Haymon Database MCP — AI access to your SQL databases",
    description:
      "A single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite. Zero runtime dependencies, read-only by default.",
    url: siteUrl,
    siteName: "Haymon Database MCP",
    type: "website",
  },
  twitter: {
    card: "summary_large_image",
    title: "Haymon Database MCP — AI access to your SQL databases",
    description:
      "A single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite. Zero runtime dependencies, read-only by default.",
  },
};

export default function Layout({ children }: LayoutProps<"/">) {
  return (
    <html lang="en" className={`${plexSans.variable} ${plexMono.variable}`} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
