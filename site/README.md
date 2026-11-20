# Psyche documentation site

Every push to `main` triggers a production build of [Psyche Docs](https://psyche-docs.vercel.app) through the [0xBuns Vercel project](https://vercel.com/0xbuns/psyche-docs). The project is connected to `OpenCoven/psyche` with the repository root as its build directory. Pull requests receive a Vercel preview.

## Update the manual

Edit `site/overview.md` for the overview or the corresponding Markdown file under `docs/` for a reference page. Update `site/build.mjs` when adding a page to the navigation. The renderer regenerates the pages, section search index, and source links from the same checkout.

## Build and publish

From the repository root, run the same commands as the [Vercel build configuration](../vercel.json).

```sh
npm ci --prefix site
npm --prefix site run check
```

The check writes `site/dist/` and exits with an error for missing linked repository files or internal heading anchors. Vercel promotes a successful production build to the public address. If the build fails, the previous deployment continues to serve. Fix the reported error and push the correction through `main` to retry.

After Vercel reports the deployment ready, check the public revision against the merge commit. From a checkout of that commit, run the following command.

```sh
test "$(curl -fsS https://psyche-docs.vercel.app/build.txt)" = "$(git rev-parse HEAD)"
```

The layout, local fonts, and OpenCoven mark follow the Coven Threads docs. Font licenses and the Threads license are included in `site/assets/`.
