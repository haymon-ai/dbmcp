import type { BaseLayoutProps } from 'fumadocs-ui/layouts/shared';
import Image from 'next/image';
import { appName, gitConfig } from './shared';

export function baseOptions(): BaseLayoutProps {
  return {
    nav: {
      title: (
        <Image
          src="/logo.svg"
          alt={appName}
          width={120}
          height={28}
          style={{ display: 'block', height: 'auto', width: 'auto' }}
        />
      ),
    },
    githubUrl: `https://github.com/${gitConfig.user}/${gitConfig.repo}`,
    searchToggle: {
      enabled: true,
    },
    themeSwitch: {
      enabled: false,
    },
  };
}
