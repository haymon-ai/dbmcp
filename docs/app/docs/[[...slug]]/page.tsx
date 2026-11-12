import type { Metadata } from 'next';
import { notFound } from 'next/navigation';
import { basePath, siteUrl } from '@/lib/shared';
import { source } from '@/lib/source';

// The docs used to live under /docs. Static export has no server redirects, so every old
// URL gets a small page that forwards to its new address (meta refresh plus a plain link).

/** Old /docs paths whose page moved somewhere other than the same path at the root. */
const moved: Record<string, string> = {
  'config-file': '/configuration/config-files',
};

function target(slug: string[] = []): string | undefined {
  const path = slug.join('/');
  if (path in moved) return moved[path];
  return source.getPage(slug) ? `/${path}` : undefined;
}

export default async function Redirect(props: PageProps<'/docs/[[...slug]]'>) {
  const { slug } = await props.params;
  const to = target(slug);
  if (!to) notFound();
  const href = `${basePath}${to === '/' ? '/' : `${to}/`}`;
  return (
    <>
      <meta httpEquiv="refresh" content={`0; url=${href}`} />
      <link rel="canonical" href={`${siteUrl}${href}`} />
      <p className="p-6">
        This page moved to <a href={href}>{href}</a>.
      </p>
    </>
  );
}

// Only the listed old URLs exist; anything else under /docs is a 404.
export const dynamicParams = false;

export function generateStaticParams() {
  return [...source.getPages().map((page) => ({ slug: page.slugs })), ...Object.keys(moved).map((path) => ({ slug: path.split('/') }))];
}

// No noindex: Google treats the instant meta refresh as a permanent redirect, and noindex
// next to rel=canonical sends mixed signals.
export const metadata: Metadata = { title: 'Moved' };
