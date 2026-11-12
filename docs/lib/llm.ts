import type { InferPageType } from 'fumadocs-core/source';
import { basePath } from './shared';
import type { source } from './source';

type Page = InferPageType<typeof source>;

/** Markdown copy of a page, served under `/llms.mdx/<slug>/content.md`. */
export function markdownUrl(page: Page): string {
  return `${basePath}/llms.mdx/${[...page.slugs, 'content.md'].join('/')}`;
}

/** A page rendered as plain Markdown for LLMs. */
export async function getLLMText(page: Page): Promise<string> {
  const raw = await page.data.getText('raw');
  const body = raw.replace(/^---\n[\s\S]*?\n---\n/, '').replaceAll('<Dbmcp />', 'dbmcp').trim();
  return `# ${page.data.title}\n\n${page.data.description ? `${page.data.description}\n\n` : ''}${body}`;
}
