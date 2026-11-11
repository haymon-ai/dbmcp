'use client';
import type { ComponentProps } from 'react';
import { useNotebookLayout } from 'fumadocs-ui/layouts/notebook';
import { useSearchContext } from 'fumadocs-ui/contexts/search';
import { Search } from 'lucide-react';
import { githubUrl } from '@/lib/layout.shared';
import { GitHubIcon } from './brand-icons';
import { MobileNav } from './mobile-nav';

/**
 * Full-width header: brand over the sidebar, search over the article, GitHub at the end.
 *
 * On mobile a second row holds the navigation toggle and breadcrumb.
 */
export function Header(props: ComponentProps<'header'>) {
  const { slots } = useNotebookLayout();
  const { setOpenSearch, hotKey } = useSearchContext();

  return (
    <header
      id="nd-subnav"
      {...props}
      className="sticky top-(--fd-docs-row-1) z-30 [grid-area:header] flex flex-col bg-fd-muted"
    >
      <div className="flex h-12 items-stretch border-b border-fd-border">
        <div className="flex shrink-0 items-center md:w-(--fd-sidebar-width)">
          {slots.navTitle && (
            <slots.navTitle className="inline-flex h-12 items-center gap-3 ps-4 pe-3 sm:pe-6" />
          )}
        </div>
        <div className="flex min-w-0 flex-1 items-center px-2 sm:px-4 md:px-6 xl:px-8">
          <button
            type="button"
            onClick={() => setOpenSearch(true)}
            aria-label="Search documentation"
            className="ms-auto flex h-8 w-8 sm:cursor-text justify-center sm:mx-auto sm:w-full sm:max-w-[820px] sm:justify-start items-center gap-2 border border-transparent hover:bg-fd-accent sm:border-fd-border sm:bg-fd-background sm:hover:bg-fd-background sm:ps-3 sm:pe-2 text-start text-sm text-subtle transition-colors hover:border-[#d5dae1]"
          >
            <Search className="size-4 shrink-0 opacity-60" />
            <span className="hidden flex-1 truncate sm:block">
              Search documentation
            </span>
            <kbd className="hidden h-5 items-center gap-0.5 border border-fd-border bg-fd-muted px-1.5 font-mono text-xs sm:inline-flex">
              {hotKey.map((k, i) => (
                // biome-ignore lint/suspicious/noArrayIndexKey: fixed hotkey list
                <span key={i}>{k.display}</span>
              ))}
            </kbd>
          </button>
        </div>
        <div className="flex shrink-0 items-stretch justify-end xl:w-(--fd-toc-width)">
          <a
            href={githubUrl}
            target="_blank"
            rel="noreferrer noopener"
            title="GitHub"
            className="flex w-12 items-center justify-center hover:bg-fd-accent"
          >
            <GitHubIcon className="size-5" />
            <span className="sr-only">GitHub repository</span>
          </a>
        </div>
      </div>
      <MobileNav />
    </header>
  );
}
