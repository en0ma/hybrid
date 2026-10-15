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

All navigation entries live in `docs.json`. Include an MDX file for every page. Preserve accurate distinctions between local Surfpool evidence, public devnet requirements and mainnet readiness.
