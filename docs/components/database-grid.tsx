import type { ReactNode } from 'react';

/**
 * Lays out a Markdown list of databases as a grid.
 *
 * Each item is `**Name** \`versions\``; the list stays plain Markdown for search and LLMs.
 */
export function DatabaseGrid({ children }: { children: ReactNode }) {
  return (
    <div className="not-prose my-4 [&_ul]:m-0 [&_ul]:grid [&_ul]:list-none [&_ul]:grid-cols-[repeat(auto-fit,minmax(170px,1fr))] [&_ul]:gap-px [&_ul]:border [&_ul]:border-fd-border [&_ul]:bg-fd-border [&_ul]:p-0 [&_li]:m-0 [&_li]:flex [&_li]:flex-col [&_li]:gap-1 [&_li]:bg-fd-background [&_li]:p-4 [&_strong]:text-base [&_strong]:leading-[22px] [&_strong]:font-semibold [&_code]:bg-transparent [&_code]:p-0 [&_code]:font-mono [&_code]:text-[13px] [&_code]:leading-[18px] [&_code]:text-fd-muted-foreground">
      {children}
    </div>
  );
}
