# GitHub Pages Deployment

Build locally:

```bash
mdbook build docs
```

The HTML output is generated in `docs/book`.

Workflow file:

- `.github/workflows/docs-pages.yml`

Repository setup:

1. `Settings -> Pages`
2. `Build and deployment -> Source: GitHub Actions`
3. Ensure branch target in workflow matches your default branch

Before first deploy, update:

- `docs/book.toml -> git-repository-url`
- `docs/book.toml -> edit-url-template`
