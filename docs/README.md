# Hybrid Docs — Mintlify

Documentation source for the Hybrid developer portal.

## Preview

```bash
npm install -g mint
cd docs
mint dev
```

## Check navigation and internal links

```bash
node docs/scripts/check-docs.mjs
```

The site uses Mintlify's **Palm** theme and a midnight/teal design system. Connect the GitHub repository to a Mintlify project and choose `docs/` as the documentation root to publish; no hosted deployment is implied by this commit.

All navigation entries live in `docs.json`. Include an MDX file for every page. Keep pages focused on protocol concepts, reproducible developer workflows and reference material. Reuse SVG exports from `assets/diagrams/` and commit their editable `.excalidraw` sources.

## Release validation

```bash
node docs/scripts/check-docs.mjs
cd docs
npx mint@latest validate
npx mint@latest broken-links
```

These commands are also required by `.github/workflows/docs.yml`. A hosted Mintlify deployment additionally requires connecting the GitHub repository through the Mintlify dashboard and selecting `docs/` as the content directory; custom domains and DNS remain environment-specific.
