import type { Metadata } from "next";
import { Inter } from "next/font/google";
import { Provider } from "@/components/provider";
import "./global.css";

const inter = Inter({
  subsets: ["latin"],
});

export const metadata: Metadata = {
  metadataBase: new URL("https://dbmcp.haymon.ai"),
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
    url: "https://dbmcp.haymon.ai",
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
    <html lang="en" className={inter.className} suppressHydrationWarning>
      <body className="flex flex-col min-h-screen">
        <Provider>{children}</Provider>
      </body>
    </html>
  );
}
