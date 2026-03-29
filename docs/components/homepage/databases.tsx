import { basePath } from '@/lib/shared';

interface SupportedItem {
  name: string;
  logo: string;
  url: string;
}

const databases: SupportedItem[] = [
  {
    name: 'MySQL',
    logo: '/logos/databases/mysql.svg',
    url: 'https://www.mysql.com',
  },
  {
    name: 'MariaDB',
    logo: '/logos/databases/mariadb.svg',
    url: 'https://mariadb.org',
  },
  {
    name: 'PostgreSQL',
    logo: '/logos/databases/postgresql.svg',
    url: 'https://www.postgresql.org',
  },
  {
    name: 'SQLite',
    logo: '/logos/databases/sqlite.svg',
    url: 'https://www.sqlite.org',
  },
];

export function Databases() {
  return (
    <section className="mx-auto max-w-4xl px-6 py-16 md:py-24">
      <div className="text-center">
        <h2 className="text-2xl font-semibold tracking-tight text-black sm:text-3xl" style={{ letterSpacing: '-0.025em' }}>
          Connect your AI to any database
        </h2>
        <p className="mx-auto mt-3 max-w-lg text-gray-500">
          One server, four backends. Choose your database — we handle the rest.
        </p>
      </div>
      <div className="mt-10 grid grid-cols-2 gap-3 sm:grid-cols-4">
        {databases.map((db) => (
          <a
            key={db.name}
            href={db.url}
            target="_blank"
            rel="noopener noreferrer"
            className="flex flex-col items-center gap-4 rounded-sm border border-black/[0.08] bg-white p-6 transition-all hover:border-black/20 hover:shadow-sm"
          >
            <img
              src={`${basePath}${db.logo}`}
              alt={`${db.name} logo`}
              className="h-12 w-12"
            />
            <span className="text-sm font-medium text-black">
              {db.name}
            </span>
          </a>
        ))}
      </div>
    </section>
  );
}
