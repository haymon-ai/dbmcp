import Link from 'fumadocs-core/link';
import { ArrowLeft, ArrowRight } from 'lucide-react';
import { githubUrl } from '@/lib/layout.shared';
import { Wordmark } from '@/components/wordmark';

/** A previous/next target: the page's title and URL. */
export interface FooterLink {
  name: string;
  url: string;
}

/** Page footer: GitHub links, previous/next cards, and the license line. */
export function PageFooter({
  editUrl,
  previous,
  next,
}: {
  editUrl: string;
  previous?: FooterLink;
  next?: FooterLink;
}) {

  const card =
    'flex min-h-24 items-center gap-4 border border-fd-border bg-fd-background px-6 py-5 text-fd-foreground transition-colors hover:border-fd-primary';

  return (
    <footer className="not-prose mt-16">
      <div className="flex flex-wrap items-center justify-between gap-4 border-t border-fd-border pt-4 text-sm">
        <a href={editUrl} target="_blank" rel="noreferrer noopener" className="text-fd-muted-foreground hover:text-fd-primary">
          Edit this page on GitHub ↗
        </a>
        <a href={`${githubUrl}/issues`} target="_blank" rel="noreferrer noopener" className="text-fd-muted-foreground hover:text-fd-primary">
          Report a docs issue ↗
        </a>
      </div>
      <div className="mt-8 grid grid-cols-1 gap-4 sm:grid-cols-2">
        <div>
          {previous && (
            <Link href={previous.url} className={card}>
              <ArrowLeft className="size-5 shrink-0 opacity-70" />
              <span className="flex min-w-0 flex-col gap-1">
                <span className="text-sm text-subtle">Previous</span>
                <span className="font-[650]">{previous.name}</span>
              </span>
            </Link>
          )}
        </div>
        <div>
          {next && (
            <Link href={next.url} className={`${card} justify-end text-end`}>
              <span className="flex min-w-0 flex-col gap-1">
                <span className="text-sm text-subtle">Next</span>
                <span className="font-[650]">{next.name}</span>
              </span>
              <ArrowRight className="size-5 shrink-0 opacity-70" />
            </Link>
          )}
        </div>
      </div>
      <p className="mt-12 text-xs text-subtle">
        © Haymon · <Wordmark /> is released under the{' '}
        <Link href="/license" className="underline">
          MIT License
        </Link>
      </p>
    </footer>
  );
}
