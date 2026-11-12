export const basePath = process.env.NEXT_PUBLIC_BASE_PATH ?? '';

export const siteUrl = 'https://dbmcp.haymon.ai';

export const appName = 'dbmcp';

/** Home page heading: what the product is. */
export const homeHeading = 'MCP server for MySQL, MariaDB, PostgreSQL, and SQLite';

/** Home page title: the product name plus the heading. */
export const homeTitle = `${appName}: ${homeHeading}`;

export const gitConfig = {
  user: 'haymon-ai',
  repo: 'dbmcp',
  branch: 'master',
};
