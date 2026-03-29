import { basePath } from '@/lib/shared';

interface SupportedItem {
  name: string;
  logo: string;
  url: string;
}

const agents: SupportedItem[] = [
  {
    name: 'Claude Desktop',
    logo: '/logos/agents/claude.svg',
    url: 'https://claude.ai',
  },
  {
    name: 'Claude Code',
    logo: '/logos/agents/claude.svg',
    url: 'https://claude.ai/code',
  },
  {
    name: 'Cursor',
    logo: '/logos/agents/cursor.svg',
    url: 'https://cursor.com',
  },
  {
    name: 'VS Code',
    logo: '/logos/agents/vscode.svg',
    url: 'https://code.visualstudio.com',
  },
  {
    name: 'Windsurf',
    logo: '/logos/agents/windsurf.svg',
    url: 'https://windsurf.com',
  },
];

export function Agents() {
  return (
    <section className="mx-auto max-w-4xl px-6 py-16 md:py-24">
      <div className="text-center">
        <h2 className="text-2xl font-semibold tracking-tight text-black sm:text-3xl" style={{ letterSpacing: '-0.025em' }}>
          Works with your AI tools
        </h2>
        <p className="mx-auto mt-3 max-w-lg text-gray-500">
          Compatible with any MCP client. Connect your favorite AI assistant in
          seconds.
        </p>
      </div>
      <div className="mt-10 grid grid-cols-2 gap-3 sm:grid-cols-3 md:grid-cols-5">
        {agents.map((agent) => (
          <a
            key={agent.name}
            href={agent.url}
            target="_blank"
            rel="noopener noreferrer"
            className="flex flex-col items-center gap-3 rounded-sm border border-black/[0.08] bg-white p-5 transition-all hover:border-black/20 hover:shadow-sm"
          >
            <img
              src={`${basePath}${agent.logo}`}
              alt={`${agent.name} logo`}
              className="h-10 w-10"
            />
            <span className="text-sm font-medium text-black">
              {agent.name}
            </span>
          </a>
        ))}
      </div>
    </section>
  );
}
