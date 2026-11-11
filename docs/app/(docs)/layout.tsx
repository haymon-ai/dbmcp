import { DocsShell } from '@/components/layout/docs-shell';

export default function Layout({ children }: LayoutProps<'/'>) {
  return <DocsShell>{children}</DocsShell>;
}
