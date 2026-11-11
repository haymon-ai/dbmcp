import { notFound } from 'next/navigation';
import { getLLMText } from '@/lib/llm';
import { source } from '@/lib/source';

export const dynamic = 'force-static';

export async function GET(_req: Request, { params }: RouteContext<'/llms.mdx/[[...slug]]'>) {
  const { slug = [] } = await params;
  if (slug.at(-1) !== 'content.md') notFound();
  const page = source.getPage(slug.slice(0, -1));
  if (!page) notFound();
  return new Response(await getLLMText(page), {
    headers: { 'Content-Type': 'text/markdown; charset=utf-8' },
  });
}

export function generateStaticParams() {
  return source.getPages().map((page) => ({ slug: [...page.slugs, 'content.md'] }));
}
