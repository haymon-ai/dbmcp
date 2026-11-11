import { docs } from 'collections/server';
import { loader } from 'fumadocs-core/source';

// Section start pages keep their real title (heading, browser tab, search, previous/next)
// but read "Overview" in the sidebar.
const overviewPages = ['clients', 'configuration', 'features', 'privacy', 'tools', 'security'].map(
  (section) => `${section}/index.mdx`,
);

// See https://fumadocs.dev/docs/headless/source-api for more info
export const source = loader({
  baseUrl: '/',
  source: docs.toFumadocsSource(),
  plugins: [
    {
      transformPageTree: {
        file: (node, filePath) =>
          filePath && overviewPages.includes(filePath) ? { ...node, name: 'Overview' } : node,
      },
    },
  ],
});
