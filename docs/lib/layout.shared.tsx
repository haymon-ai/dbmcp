import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { appName, basePath, docsRoute, gitConfig } from './shared';

export const githubUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      url: docsRoute,
      title: (
        <>
          <img src={`${basePath}/logo.svg`} alt="dbmcp" className="block h-[26px] w-auto" />
          <span className="text-base font-semibold whitespace-nowrap text-fd-foreground">
            {appName}
          </span>
        </>
      ),
    },
    themeSwitch: {
      enabled: false,
    },
  };
}
