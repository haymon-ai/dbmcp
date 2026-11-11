import defaultMdxComponents from 'fumadocs-ui/mdx';
import { CodeBlockTabs } from 'fumadocs-ui/components/codeblock';
import type { MDXComponents } from 'mdx/types';
import type { ComponentProps } from 'react';
import { cn } from '@/lib/cn';
import { DocsCodeBlock } from '@/components/code-block';
import { DatabaseGrid } from '@/components/database-grid';
import { Demo } from '@/components/demo';

/** Code tab groups with the same default tab share one remembered choice (e.g. bin / npx). */
function DocsCodeBlockTabs(props: ComponentProps<typeof CodeBlockTabs>) {
  return (
    <CodeBlockTabs
      groupId={`code-${props.defaultValue}`}
      persist
      {...props}
      className={cn('code-tabs', props.className)}
    />
  );
}

export function getMDXComponents(components?: MDXComponents) {
  return {
    ...defaultMdxComponents,
    pre: DocsCodeBlock,
    CodeBlockTabs: DocsCodeBlockTabs,
    DatabaseGrid,
    Demo,
    ...components,
  } satisfies MDXComponents;
}

export const useMDXComponents = getMDXComponents;

declare global {
  type MDXProvidedComponents = ReturnType<typeof getMDXComponents>;
}
