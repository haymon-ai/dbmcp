import type { Metadata } from 'next';
import { DocsDescription, DocsPage, DocsTitle } from 'fumadocs-ui/layouts/notebook/page';
import { DocsShell } from '@/components/layout/docs-shell';

export const metadata: Metadata = { title: 'Page not found' };

/** 404 inside the regular docs chrome, so readers can search or navigate on. */
export default function NotFound() {
  return (
    <DocsShell>
      <DocsPage
        breadcrumb={{ enabled: false }}
        footer={{ enabled: false }}
        className="mx-auto w-full max-w-[884px] gap-0 pb-24 *:max-w-none xl:pt-[26px]"
      >
        <p className="font-mono text-sm text-subtle">404</p>
        <DocsTitle className="mb-3 text-[42px] leading-[50px] font-[650]">Page not found</DocsTitle>
        <DocsDescription className="mb-8 text-xl leading-7 text-fd-muted-foreground">
          This page does not exist or has moved.
        </DocsDescription>
      </DocsPage>
    </DocsShell>
  );
}
