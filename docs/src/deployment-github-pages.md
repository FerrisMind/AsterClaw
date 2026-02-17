# Деплой на GitHub Pages

## Локальная сборка книги

```bash
mdbook build docs
```

Артефакт HTML будет в `docs/book`.

## CI/CD workflow

В репозитории добавлен workflow:

- `.github/workflows/docs-pages.yml`

Он:

1. Устанавливает `mdbook`
2. Собирает `mdbook build docs`
3. Публикует артефакт в GitHub Pages

## Настройка репозитория

1. Откройте `Settings -> Pages`
2. В `Build and deployment` выберите `GitHub Actions`
3. Убедитесь, что default branch соответствует `main` (или измените workflow)

## Важные правки перед первым деплоем

Обновите `docs/book.toml`:

- `git-repository-url`
- `edit-url-template`

Поставьте ваш реальный `OWNER/REPO`.
