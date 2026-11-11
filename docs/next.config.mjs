import { createMDX } from 'fumadocs-mdx/next';
import { PHASE_PRODUCTION_BUILD } from 'next/constants.js';

const withMDX = createMDX();

const basePath = process.env.PAGES_BASE_PATH || '';

/** @type {import('next').NextConfig} */
const config = {
  reactStrictMode: true,
  trailingSlash: true,
  images: { unoptimized: true },
  basePath,
  assetPrefix: basePath ? `${basePath}/` : '',
  env: { NEXT_PUBLIC_BASE_PATH: basePath },
};

// Static export only for `next build`: in `next dev`, export mode turns every unknown URL
// under a dynamic route into a server error instead of rendering app/not-found.tsx.
export default (phase) =>
  withMDX({ ...config, output: phase === PHASE_PRODUCTION_BUILD ? 'export' : undefined });
