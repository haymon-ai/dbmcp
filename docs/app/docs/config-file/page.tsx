import { basePath, docsRoute } from '@/lib/shared';

const target = `${basePath}${docsRoute}/configuration/config-files/`;

/** The old Config File page moved; static export has no server redirects. */
export default function ConfigFileRedirect() {
  return (
    <>
      <meta httpEquiv="refresh" content={`0; url=${target}`} />
      <link rel="canonical" href={target} />
      <p className="p-6">
        This page moved to <a href={target}>Config files</a>.
      </p>
    </>
  );
}
