# Deploy no GitHub Pages

Build local:

```bash
mdbook build docs
```

Saída HTML em `docs/book`.

Workflow:

- `.github/workflows/docs-pages.yml`

Configuração do repositório:

1. `Settings -> Pages`
2. `Build and deployment -> Source: GitHub Actions`
3. Garanta que a branch no workflow está correta

Antes do primeiro deploy, ajuste:

- `docs/book.toml -> git-repository-url`
- `docs/book.toml -> edit-url-template`
