import type { ReactNode } from 'react';
import { DocsLayout } from 'fumadocs-ui/layouts/notebook';
import { Header } from '@/components/layout/header';
import { baseOptions } from '@/lib/layout.shared';
import { source } from '@/lib/source';

/** The docs chrome (header, sidebar, mobile nav), shared by the docs pages and the 404 page. */
export function DocsShell({ children }: { children: ReactNode }) {
  const base = baseOptions();
  return (
    <DocsLayout
      tree={source.getPageTree()}
      {...base}
      nav={{ ...base.nav, mode: 'top' }}
      sidebar={{ collapsible: false, className: 'bg-fd-background border-e border-fd-border' }}
      slots={{ header: Header }}
    >
      {children}
    </DocsLayout>
  );
}
