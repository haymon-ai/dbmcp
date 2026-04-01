import Link from 'next/link';

const gitHubUrl = 'https://github.com/haymon-ai/database';

export function Hero() {
  return (
    <section className="mx-auto w-full max-w-4xl px-6 pt-16 md:pt-28 pb-16 text-center">
      <Link
        href="/docs"
        className="inline-flex items-center gap-2 rounded-sm border border-black/10 px-4 py-1.5 text-sm text-gray-600 transition-colors hover:border-black/20 hover:bg-gray-50"
      >
        <span className="inline-block h-2 w-2 rounded-full bg-[#151715]" />
        Open source MCP server
        <span aria-hidden="true">&rarr;</span>
      </Link>

      <h1 className="mt-8 text-4xl font-semibold tracking-tight text-black sm:text-5xl md:text-6xl" style={{ letterSpacing: '-0.025em', lineHeight: 1 }}>
        Your databases,{' '}
        <br className="hidden sm:block" />
        meet your AI
      </h1>

      <p className="mx-auto mt-6 max-w-xl text-lg text-gray-500">
        A single-binary MCP server for MySQL, MariaDB, PostgreSQL, and SQLite.
        Zero runtime dependencies. Zero context-switching.
      </p>

      <div className="mt-8 flex flex-col sm:flex-row items-center justify-center gap-3">
        <Link
          href="/docs"
          className="inline-flex items-center justify-center rounded-sm bg-[#151715] px-5 py-2.5 text-sm font-medium text-white transition-colors hover:bg-[#2a2c2a]"
        >
          Get started
        </Link>
        <a
          href={gitHubUrl}
          target="_blank"
          rel="noopener noreferrer"
          className="inline-flex items-center justify-center rounded-sm border border-black/15 bg-white px-5 py-2.5 text-sm font-medium text-[#141e12] transition-colors hover:bg-gray-50"
        >
          View on GitHub
        </a>
      </div>
    </section>
  );
}
