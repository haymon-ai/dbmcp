import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { appName, basePath, gitConfig } from './shared';

export const githubUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      url: '/',
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
