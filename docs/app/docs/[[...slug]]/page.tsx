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
import { basePath, docsRoute, gitConfig, siteUrl } from '@/lib/shared';

export default async function Page(props: PageProps<'/docs/[[...slug]]'>) {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  const MDX = page.data.body;
  const isIntro = page.slugs.length === 0;
  // No Copy Markdown / Open actions on the license text.
  const showActions = page.url !== `${docsRoute}/license`;
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
      // The start page has no "On this page" list.
      tableOfContent={{ style: 'clerk', enabled: !isIntro }}
      tableOfContentPopover={{
        enabled: !isIntro,
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
          <div className="flex flex-wrap items-start justify-between gap-4">
            <DocsTitle className="mb-3 min-w-0 text-[42px] leading-[50px] font-[650]">
              {page.data.title}
            </DocsTitle>
            {showActions && (
              <PageActions markdownUrl={markdownUrl(page)} pageUrl={`${siteUrl}${basePath}${page.url}/`} />
            )}
          </div>
          <DocsDescription className="mb-8 text-xl leading-7 text-fd-muted-foreground">
            {page.data.description}
          </DocsDescription>
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

export async function generateStaticParams() {
  return source.generateParams();
}

export async function generateMetadata(props: PageProps<'/docs/[[...slug]]'>): Promise<Metadata> {
  const params = await props.params;
  const page = source.getPage(params.slug);
  if (!page) notFound();

  return {
    title: page.data.title,
    description: page.data.description,
  };
}
