'use client';
import {
  SearchDialog,
  SearchDialogClose,
  SearchDialogContent,
  SearchDialogFooter,
  SearchDialogHeader,
  SearchDialogIcon,
  SearchDialogInput,
  SearchDialogList,
  SearchDialogListItem,
  SearchDialogOverlay,
  type SharedProps,
} from 'fumadocs-ui/components/dialog/search';
import { useDocsSearch } from 'fumadocs-core/search/client';
import { useI18n } from 'fumadocs-ui/contexts/i18n';
import { basePath } from '@/lib/shared';

export default function DefaultSearchDialog(props: SharedProps) {
  const { locale } = useI18n(); // (optional) for i18n
  const { search, setSearch, query } = useDocsSearch({
    type: 'static',
    from: `${basePath}/api/search`,
    locale,
  });

  return (
    <SearchDialog search={search} onSearchChange={setSearch} isLoading={query.isLoading} {...props}>
      {/* Light, unblurred overlay; overlay and dialog appear and close without animation. */}
      <SearchDialogOverlay className="bg-fd-muted/80 backdrop-blur-none data-[state=closed]:animate-none data-[state=open]:animate-none" />
      <SearchDialogContent className="max-w-[640px] md:top-20 data-[state=closed]:animate-none data-[state=open]:animate-none border-fd-border shadow-[0_12px_32px_rgba(1,17,44,0.18)]">
        <SearchDialogHeader>
          <SearchDialogIcon />
          <SearchDialogInput />
          <SearchDialogClose />
        </SearchDialogHeader>
        <SearchDialogList
          items={query.data !== 'empty' ? query.data : null}
          Item={(itemProps) => (
            <SearchDialogListItem
              {...itemProps}
              className="cursor-pointer aria-selected:bg-[#f0efff] aria-selected:shadow-[inset_3px_0_0_var(--color-fd-primary)]"
            />
          )}
        />
        <SearchDialogFooter className="flex h-10 items-center gap-4 border-t border-fd-border bg-fd-muted px-4 text-xs text-fd-muted-foreground">
          <span>↑ ↓ to navigate</span>
          <span>↵ to select</span>
          <span>Esc to close</span>
        </SearchDialogFooter>
      </SearchDialogContent>
    </SearchDialog>
  );
}
