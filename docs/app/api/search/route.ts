import { createFromSource } from "fumadocs-core/search/server";
import { plainWordmark } from "@/lib/llm";
import { source } from "@/lib/source";

export const revalidate = false;

// The structured data keeps inline JSX as text, and the search dialog would render
// `<Dbmcp />` as an element wrapping the rest of the result, so index plain text instead.
const search = createFromSource(source, {
  buildIndex: (page) => {
    const { headings, contents } = page.data.structuredData;
    return {
      title: page.data.title,
      description: page.data.description,
      url: page.url,
      id: page.url,
      structuredData: {
        headings: headings.map((h) => ({
          ...h,
          content: plainWordmark(h.content),
        })),
        contents: contents.map((c) => ({
          ...c,
          content: plainWordmark(c.content),
        })),
      },
    };
  },
});

export const GET = search.staticGET;
