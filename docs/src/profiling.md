# Профилирование и NFR

В репозитории есть скрипты для измерений в `scripts/`.

## Базовый замер

```powershell
pwsh scripts/nfr_baseline.ps1
```

Скрипт показывает:

- размер бинарника
- startup latency до ready
- RSS

## Сравнение с baseline

```powershell
pwsh scripts/nfr-harness.ps1
```

Скрипт формирует отчёт в `target/nfr/nfr-results.json`.

## Точечный профайлинг инструментов

```powershell
pwsh scripts/simply-profile.ps1
```

Отчёт:

- `target/simply-profile/results.json`

## Практика

- Фиксируйте baseline перед изменениями
- Меняйте 1-2 параметра за итерацию
- Сравнивайте одинаковые сценарии нагрузки
