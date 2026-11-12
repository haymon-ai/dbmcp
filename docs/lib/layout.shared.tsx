import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import { appName, basePath, gitConfig } from './shared';

export const githubUrl = `https://github.com/${gitConfig.user}/${gitConfig.repo}`;

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      url: '/',
      title: (
        <img src={`${basePath}/logo.svg`} alt={appName} className="block h-[26px] w-auto" />
      ),
    },
    themeSwitch: {
      enabled: false,
    },
  };
}
