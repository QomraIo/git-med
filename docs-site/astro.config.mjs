import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

export default defineConfig({
  site: 'https://qomraio.github.io',
  base: '/git-med',

  integrations: [
    starlight({
      title: 'git-med',
      description: 'Distributed Version Control for Medical Imaging Datasets',
      social: [
        {
          label: 'GitHub',
          icon: 'github',
          href: 'https://github.com/QomraIo/git-med',
        },
      ],
      sidebar: [
        {
          label: 'Getting Started',
          items: [
            { label: 'Overview & Quickstart', slug: 'getting-started' },
            { label: 'Authentication & RBAC', slug: 'authentication' },
          ],
        },
        {
          label: 'CLI Commands Reference',
          items: [{ autogenerate: { directory: 'commands' } }],
        },
        {
          label: 'Medical Imaging Data',
          items: [
            { label: 'DICOM Tag Rules', slug: 'dcm_tags_rules' },
          ],
        },
      ],
      customCss: [],
    }),
  ],
});
