'use client';
import { useEffect, useState } from 'react';
import Link from 'fumadocs-core/link';
import { useTreeContext, useTreePath } from 'fumadocs-ui/contexts/tree';
import { ChevronRight, Menu, X } from 'lucide-react';
import { cn } from '@/lib/cn';

/**
 * Mobile navigation bar: a menu toggle with the page breadcrumb.
 *
 * The toggle opens the page tree as a full-width panel below the header.
 */
export function MobileNav() {
  const { full } = useTreeContext();
  // [separator, page] for a sidebar page; empty on Introduction, which the tree hides.
  const path = useTreePath();
  const page = path.at(-1);
  const group = path.find((node) => node.type === 'separator');
  const [open, setOpen] = useState(false);

  // Close on navigation.
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs when the path changes
  useEffect(() => setOpen(false), [path]);

  useEffect(() => {
    if (!open) return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(false);
    };
    window.addEventListener('keydown', onKey);
    document.body.style.overflow = 'hidden';
    return () => {
      window.removeEventListener('keydown', onKey);
      document.body.style.overflow = '';
    };
  }, [open]);

  return (
    <div className="flex h-12 items-center gap-3 border-b border-fd-border px-4 md:hidden">
      <button
        type="button"
        aria-label={open ? 'Close navigation' : 'Open navigation'}
        aria-expanded={open}
        onClick={() => setOpen(!open)}
        className="-ms-1 flex size-8 items-center justify-center hover:bg-fd-accent"
      >
        {open ? <X className="size-5" /> : <Menu className="size-5" />}
      </button>
      <nav
        aria-label="Breadcrumb"
        className="flex min-w-0 items-center gap-2 text-sm"
      >
        {group && (
          <>
            <span className="shrink-0 text-fd-muted-foreground">{group.name}</span>
            <ChevronRight className="size-4 shrink-0 text-fd-muted-foreground" />
          </>
        )}
        <span className="truncate font-semibold">{page ? page.name : 'Introduction'}</span>
      </nav>
      {open && (
        <div className="fixed inset-x-0 top-(--fd-header-height) bottom-0 z-40 overflow-y-auto bg-fd-background pb-8">
          <kbd className="absolute top-6 right-4 border border-fd-border bg-fd-muted px-1.5 font-mono text-xs text-fd-muted-foreground">
            esc
          </kbd>
          {full.children.map((node) =>
            node.type === 'separator' ? (
              <p
                key={node.$id ?? String(node.name)}
                className="px-4 pt-6 pb-2 text-xs font-semibold tracking-[0.32px]"
              >
                {node.name}
              </p>
            ) : node.type === 'page' ? (
              <Link
                key={node.url}
                href={node.url}
                className={cn(
                  'flex h-10 items-center border-s-3 border-transparent ps-[13px] pe-4 text-[15px] text-fd-muted-foreground hover:bg-fd-accent hover:text-fd-foreground',
                  node === page &&
                    'border-fd-primary bg-fd-accent font-semibold text-fd-foreground',
                )}
              >
                {node.name}
              </Link>
            ) : null,
          )}
        </div>
      )}
    </div>
  );
}
