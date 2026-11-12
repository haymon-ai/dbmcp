import { source } from '@/lib/source';
import {
  DocsBody,
  DocsDescription,
  DocsPage,
  DocsTitle,
} from 'fumadocs-ui/layouts/notebook/page';
import { notFound } from 'next/navigation';
import { getMDXComponents } from '@/components/mdx';
import type { Metadata } from 'next';
import { createRelativeLink } from 'fumadocs-ui/mdx';
import { PageActions } from '@/components/layout/page-actions';
import { flattenTree, type Item } from 'fumadocs-core/page-tree';
import { type FooterLink, PageFooter } from '@/components/layout/page-footer';
import { githubUrl } from '@/lib/layout.shared';
import { markdownUrl } from '@/lib/llm';
import { basePath, gitConfig, siteUrl } from '@/lib/shared';

export default async function Page(props: PageProps<'/[[...slug]]'>) {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const isIntro = page.slugs.length === 0;
  const isLicense = page.url === '/license';
  // No Copy Markdown / Open actions on the license text.
  const showActions = !isLicense;
  // The start page and the license have no "On this page" list.
  const showToc = !isIntro && !isLicense;
  const editUrl = `${githubUrl}/blob/${gitConfig.branch}/docs/content/docs/${page.path}`;

  // Previous/next follow the sidebar order but show page titles, so a section start page
  // reads "Configuration" there, not its sidebar label "Overview".
  const items = flattenTree(source.getPageTree().children);
  const idx = items.findIndex((item) => item.url === page.url);
  const link = (item?: Item): FooterLink | undefined =>
    item && { name: source.getNodePage(item)?.data.title ?? String(item.name), url: item.url };
  // Introduction is hidden from the sidebar tree (idx -1), so it heads the chain.
  const intro = source.getPage([]);
  const previous = idx === 0 && intro ? { name: intro.data.title, url: intro.url } : link(items[idx - 1]);
  const next = link(items[idx + 1]);

  return (
    <DocsPage
      toc={page.data.toc}
      full={page.data.full}
      tableOfContent={{ style: 'clerk', enabled: showToc }}
      tableOfContentPopover={{
        enabled: showToc,
        // Solid white bar instead of the translucent blur, with a primary-coloured progress circle.
        container: { className: '[&>header]:bg-fd-background [&>header]:backdrop-blur-none' },
        trigger: { className: '[&_[role=progressbar]]:text-fd-primary' },
      }}
      breadcrumb={{ enabled: false }}
      footer={{ enabled: false }}
      className="mx-auto w-full max-w-[884px] gap-0 pb-24 *:max-w-none xl:pt-[26px]"
    >
      {/* The start page opens with the demo, without a title or description. */}
      {!isIntro && (
        <>
          {/* Actions sit beside the title from 1350px, where the column (with sidebar and TOC)
              still fits the longest title on one line; below, under the description.
              Phones get a divider under the header. */}
          <div className="mb-8 border-b border-fd-border pb-6 md:border-0 md:pb-0">
            <div className="grid grid-cols-1 min-[1350px]:grid-cols-[minmax(0,1fr)_auto] min-[1350px]:gap-x-6">
              <DocsTitle className="mb-3 min-w-0 text-[42px] leading-[50px] font-[650]">
                {page.data.title}
              </DocsTitle>
              <DocsDescription className="mb-0 min-w-0 text-xl leading-7 text-fd-muted-foreground min-[1350px]:col-span-2 min-[1350px]:row-start-2">
                {page.data.description}
              </DocsDescription>
              {showActions && (
                // Stacked, the Copy Markdown label lines up with the text (cancels the button's padding).
                <div className="-ms-2.5 mt-3 min-[1350px]:col-start-2 min-[1350px]:row-start-1 min-[1350px]:ms-0 min-[1350px]:mt-[11px]">
                  <PageActions markdownUrl={markdownUrl(page)} pageUrl={`${siteUrl}${basePath}${page.url === '/' ? '/' : `${page.url}/`}`} />
                </div>
              )}
            </div>
          </div>
        </>
      )}
      <DocsBody>
        <MDX
          components={getMDXComponents({
            // this allows you to link to other pages with relative file paths
            a: createRelativeLink(source, page),
          })}
        />
      </DocsBody>
      <PageFooter editUrl={editUrl} previous={previous} next={next} />
    </DocsPage>
  );
}

// Only the generated pages exist; any other slug renders app/not-found.tsx (in dev too).
export const dynamicParams = false;

export async function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata(props: PageProps<'/[[...slug]]'>): Promise<Metadata> {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  return {
    title: page.data.title,
    description: page.data.description,
    alternates: {
      // metadataBase makes these absolute; trailingSlash adds the slash.
      canonical: page.url,
      types: { 'text/markdown': markdownUrl(page) },
    },
    // A page's openGraph replaces the layout's whole object, so siteName is repeated here.
    openGraph: {
      url: page.url,
      siteName: 'Haymon Database MCP',
      type: page.slugs.length === 0 ? 'website' : 'article',
    },
  };
}
