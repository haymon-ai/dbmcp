import { basePath, docsRoute } from '@/lib/shared';

const target = `${basePath}${docsRoute}/`;

/** `/` forwards to the Introduction page; static export has no server redirects. */
export default function HomePage() {
  return (
    <>
      <meta httpEquiv="refresh" content={`0; url=${target}`} />
      <link rel="canonical" href={target} />
      <p className="p-6">
        Redirecting to <a href={target}>the documentation</a>…
      </p>
    </>
  );
}
