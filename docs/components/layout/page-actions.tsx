'use client';
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from 'fumadocs-ui/components/ui/popover';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';
import { Check, ChevronDown, Copy, ExternalLink } from 'lucide-react';
import {
  ClaudeIcon,
  CursorIcon,
  MarkdownIcon,
  OpenAIIcon,
} from './brand-icons';

const cache = new Map<string, Promise<string>>();

function fetchMarkdown(url: string): Promise<string> {
  let text = cache.get(url);
  if (!text) {
    text = fetch(url).then((res) => {
      if (!res.ok) throw new Error(`Failed to fetch ${url}: ${res.status}`);
      return res.text();
    });
    // Keep only successful fetches so a later click retries.
    text.catch(() => cache.delete(url));
    cache.set(url, text);
  }
  return text;
}

const assistants = [
  {
    label: 'Open in ChatGPT',
    base: 'https://chatgpt.com/?q=',
    Icon: OpenAIIcon,
  },
  {
    label: 'Open in Claude',
    base: 'https://claude.ai/new?q=',
    Icon: ClaudeIcon,
  },
  {
    label: 'Open in Cursor',
    base: 'https://cursor.com/link/prompt?text=',
    Icon: CursorIcon,
  },
];

/** Copy Markdown button and the "Open" menu shown beside a page title. */
export function PageActions({
  markdownUrl,
  pageUrl,
}: {
  markdownUrl: string;
  pageUrl: string;
}) {
  const [checked, onCopy] = useCopyButton(async () => {
    // Pass the pending fetch to the clipboard so the write starts within the click (Safari).
    await navigator.clipboard.write([
      new ClipboardItem({ 'text/plain': fetchMarkdown(markdownUrl) }),
    ]);
  });
  const prompt = encodeURIComponent(
    `Read ${pageUrl} so I can ask questions about it.`,
  );
  const row =
    'flex w-full items-start gap-3 px-4 py-2.5 text-start text-sm text-fd-foreground hover:bg-fd-muted';

  return (
    <div className="not-prose mt-[11px] flex shrink-0 items-center gap-1">
      <button
        type="button"
        onClick={onCopy}
        className="flex h-7 items-center gap-1.5 px-2.5 text-[13px] text-fd-foreground whitespace-nowrap hover:bg-fd-accent"
      >
        {checked ? <Check className="size-3.5" /> : <Copy className="size-3.5" />}
        {checked ? 'Copied Markdown' : 'Copy Markdown'}
      </button>
      <Popover>
        <PopoverTrigger className="flex h-7 items-center gap-1.5 border border-fd-border bg-fd-background px-2.5 text-[13px] text-fd-foreground whitespace-nowrap hover:bg-fd-muted">
          Open <ChevronDown className="size-3.5" />
        </PopoverTrigger>
        <PopoverContent
          align="end"
          className="w-[300px] border-fd-border bg-fd-popover p-1 shadow-[0_2px_6px_rgba(0,0,0,0.12)] backdrop-blur-none"
        >
          <a
            href={markdownUrl}
            target="_blank"
            rel="noreferrer noopener"
            className={row}
          >
            <MarkdownIcon className="mt-px size-4 shrink-0" />
            <span className="flex flex-1 flex-col gap-0.5">
              <span>View as Markdown</span>
              <span className="text-xs text-subtle">
                View this page as plain text
              </span>
            </span>
            <ExternalLink className="mt-px size-4 shrink-0 opacity-70" />
          </a>
          {assistants.map((a) => (
            <a
              key={a.label}
              href={a.base + prompt}
              target="_blank"
              rel="noreferrer noopener"
              className={row}
            >
              <a.Icon className="mt-px size-4 shrink-0" />
              <span className="flex flex-1 flex-col gap-0.5">
                <span>{a.label}</span>
                <span className="text-xs text-subtle">
                  Ask questions about this page
                </span>
              </span>
              <ExternalLink className="mt-px size-4 shrink-0 opacity-70" />
            </a>
          ))}
        </PopoverContent>
      </Popover>
    </div>
  );
}
