import { DocsShell } from '@/components/layout/docs-shell';

export default function Layout({ children }: LayoutProps<'/docs'>) {
  return <DocsShell>{children}</DocsShell>;
}
