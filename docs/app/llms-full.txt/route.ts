import { getLLMText } from '@/lib/llm';
import { source } from '@/lib/source';

export const dynamic = 'force-static';

export async function GET() {
  const pages = await Promise.all(source.getPages().map(getLLMText));
  return new Response(pages.join('\n\n'));
}
