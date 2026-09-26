// @ts-check
import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';

// https://astro.build/config
export default defineConfig({
	site: 'https://lacodda.github.io',
	base: '/furca',
	vite: {
		build: {
			rolldownOptions: {
				// Astro puts a "use astro:head-inject" directive into every MDX page
				// that imports a component, and the bundler warns that it may drop
				// it. The directive is Astro's own, read by Astro before bundling;
				// only that one warning is silenced, every other still prints.
				onwarn(warning, warn) {
					if (warning.code === 'MODULE_LEVEL_DIRECTIVE' && warning.message.includes('astro:head-inject')) return;
					warn(warning);
				},
			},
		},
	},
	integrations: [
		starlight({
			title: 'furca',
			// The 404 page is a content page (src/content/docs/404.md): Starlight's own
			// route looks for that entry and warns on every build when it is missing.
			disable404Route: true,
			description: 'A fast git client that stays out of your way.',
			logo: {
				src: './src/assets/logo.svg',
				alt: 'furca',
			},
			favicon: '/favicon.svg',
			customCss: ['./src/styles/brand.css'],
			head: [
				{ tag: 'link', attrs: { rel: 'apple-touch-icon', href: '/furca/apple-touch-icon.png' } },
				{ tag: 'meta', attrs: { property: 'og:image', content: 'https://raw.githubusercontent.com/lacodda/furca/main/assets/social-preview.png' } },
				{ tag: 'meta', attrs: { name: 'twitter:card', content: 'summary_large_image' } },
			],
			social: [{ icon: 'github', label: 'GitHub', href: 'https://github.com/lacodda/furca' }],
			editLink: {
				baseUrl: 'https://github.com/lacodda/furca/edit/main/docs/',
			},
			sidebar: [
				{ label: 'Getting Started', slug: 'getting-started' },
				{
					label: 'Concepts',
					items: [{ autogenerate: { directory: 'concepts' } }],
				},
				{
					label: 'Guides',
					items: [{ autogenerate: { directory: 'guides' } }],
				},
				{
					label: 'Reference',
					items: [{ autogenerate: { directory: 'reference' } }],
				},
			],
		}),
	],
});
