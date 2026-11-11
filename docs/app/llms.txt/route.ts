import { llms } from 'fumadocs-core/source';
import { appName, docsRoute } from '@/lib/shared';
import { source } from '@/lib/source';

export const dynamic = 'force-static';

export function GET() {
  // The page tree hides Introduction from the sidebar; list it first here.
  const intro = source.getPage([]);
  const head = `# ${appName}\n\n- [Introduction](${docsRoute}): ${intro?.data.description ?? ''}\n`;
  const index = llms(source).index().replace(/^# .*\n\n/, '');
  return new Response(head + index);
}
