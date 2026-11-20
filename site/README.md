# Psyche documentation site

The site uses the Coven Threads documentation layout, local fonts, and OpenCoven mark. The overview lives in `overview.md`. The other pages render the selected Markdown files from the latest project revision that changed files outside the site under `../docs/`. Each reference page links to that revision on GitHub.

Run these commands from `site/` to build and validate the static site.

```sh
npm ci
npm run check
```

Preview `dist/` with a static HTTP server. To publish the validated output to the existing 0xBuns project, run the following from `site/`.

```sh
vercel link --yes --project psyche-docs --scope 0xbuns
vercel deploy --prod --scope 0xbuns
```

The Vercel configuration serves the local build. Rebuild and deploy after documentation changes. Font licenses and the Threads license are included in `assets/`.
