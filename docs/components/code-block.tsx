'use client';
import { type ComponentProps, type CSSProperties, useRef } from 'react';
import { CodeBlock, type CodeBlockProps, Pre } from 'fumadocs-ui/components/codeblock';
import { useCopyButton } from 'fumadocs-ui/utils/use-copy-button';
import { Check, Copy } from 'lucide-react';
import { cn } from '@/lib/cn';

/** Copy icon button; `className` places it in the title bar or over the code (set by CodeBlock). */
function CopyIcon({ className }: ComponentProps<'div'>) {
  const ref = useRef<HTMLButtonElement>(null);
  const [checked, onClick] = useCopyButton(async () => {
    const pre = ref.current?.closest('figure')?.querySelector('pre');
    if (!pre) return;
    const clone = pre.cloneNode(true) as HTMLElement;
    for (const node of clone.querySelectorAll('.nd-copy-ignore')) node.replaceWith('\n');
    await navigator.clipboard.writeText(clone.textContent ?? '');
  });

  return (
    <div className={className}>
      <button
        ref={ref}
        type="button"
        onClick={onClick}
        aria-label={checked ? 'Copied' : 'Copy code'}
        className="flex size-6 items-center justify-center text-code-muted hover:bg-code-hover hover:text-code-fg"
      >
        {checked ? <Check className="size-4" /> : <Copy className="size-4" />}
      </button>
    </div>
  );
}

/** Code block on the navy panel with a copy icon, used for MDX `pre`. */
export function DocsCodeBlock(props: CodeBlockProps) {
  return (
    <CodeBlock
      {...props}
      allowCopy={false}
      Actions={CopyIcon}
      className={cn('rounded-[3px] border-0 bg-code text-code-fg shadow-none', props.className)}
      // Without a title the copy icon sits over the top-right of the code; keep lines clear of it.
      viewportProps={props.title ? undefined : { style: { '--padding-right': '44px' } as CSSProperties }}
    >
      <Pre>{props.children}</Pre>
    </CodeBlock>
  );
}
