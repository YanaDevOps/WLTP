# WLTP - Modern WinMTR for Windows

**Portable network diagnostic tool with smart interpretation** - современная альтернатива WinMTR для Windows с понятным интерфейсом и объяснением результатов.

## ✨ Возможности

- 🔍 **Network Diagnostics** - ICMP traceroute с непрерывным измерением
- 🧠 **Smart Interpretation** - умный анализ проблем (отличает rate-limiting от реальной потери пакетов)
- 🎨 **Modern UI** - чистый графический интерфейс на React + Tailwind CSS
- 🌙 **Theme Support** - System/Light/Dark темы
- 📊 **Export Reports** - HTML и JSON экспорт для тикетов в поддержку
- 🚀 **Portable** - один EXE файл, ничего не нужно устанавливать

## 🚀 Как получить portable EXE

### Автоматическая сборка (рекомендуется)

**Всё готово!** GitHub Actions автоматически соберёт EXE файл после первого пуши в репозиторий.

1. **Перейдите в Actions**: https://github.com/ВАШ_ЮЗЕРНЕЙМ/WLTP/actions
2. **Дождитесь завершения сборки** (обычно 5-10 минут)
3. **Скачайте artefacts**: В разделе "Artifacts" внизу страницы сборки
4. **Распакуйте и запускайте** `WLTP.exe`

### Ручная сборка (если нужно)

```bash
# Установить зависимости
npm install

# Собрать приложение
npm run tauri build

# EXE будет здесь:
# src-tauri/target/release/WLTP.exe
```

**Требования для ручной сборки:**
- Windows 10/11
- Visual Studio Build Tools (Desktop development with C++)
- Rust 1.70+

## 📖 Использование

### Базовый запуск

1. Запустите `WLTP.exe`
2. Введите хост или IP (например: `google.com` или `8.8.8.8`)
3. Нажмите "Start Trace"
4. Смотрите результаты в реальном времени

### Экспорт отчётов

- **HTML** - для отправки в поддержку (включает интерпретацию и метрики)
- **JSON** - для технического анализа и интеграции

## 🧠 Что такое умная интерпретация?

WLTP не просто показывает цифры - он **объясняет** что происходит:

| Проблема | Обычный traceroute | WLTP |
|----------|-------------------|------|
| Промежуточный хоп не отвечает | `*** Request timed out` | "Hop not responding (likely normal) - many routers deprioritize ICMP" |
| Потеря пакетов в середине | "Loss: 50%" | "Packet loss starting here - likely ICMP rate limiting" (если следующие хопы нормальные) |
| Высокая задержка | "Latency: 300ms" | "High latency starting at hop 5 - congested link at this segment" |

## 🎨 Интерфейс

### Основные экраны

**Diagnostic View:**
- Поле ввода хоста
- Кнопка Start/Stop
- Таблица хопов с цветовой кодировкой
- Summary карточка с диагностикой

**Hops Table:**
- Status (✓⚠✗?) индикаторы
- Host и IP
- Loss%, Sent, Recv, Best, Avg, Worst, Last, Jitter
- Интерпретация для каждого хопа

**Settings:**
- Theme (System/Light/Dark)
- Explanation Level (Simple/Detailed)
- Measurement parameters (interval, max hops, timeout)

## 🏗️ Архитектура

```
WLTP/
├── src-tauri/          # Rust backend
│   ├── src/
│   │   ├── traceroute.rs       # ICMP implementation
│   │   ├── interpretation.rs   # Diagnostic engine
│   │   ├── types.rs            # Core types
│   │   └── commands.rs         # Tauri commands
│   └── Cargo.toml
├── src/                # React frontend
│   ├── App.tsx         # Main component
│   ├── lib/tauri.ts    # API wrapper
│   └── types/
│       └── global.d.ts # Type definitions
└── package.json
```

## 🔧 Технологии

- **Backend**: Rust + Tauri 2.x
- **Frontend**: React 18 + TypeScript + Vite
- **Styling**: Tailwind CSS
- **Build**: GitHub Actions (automatic)

## 📋 Требования к системе

**Для запуска EXE:**
- Windows 10/11 (64-bit)
- Администраторские права (для raw ICMP sockets)
- ~10 MB свободного места

**Для разработки:**
- Node.js 20+
- Rust 1.70+
- Visual Studio Build Tools

## 🤝 Участие в разработке

1. Fork репозиторий
2. Создайте feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit изменения (`git commit -m 'Add some AmazingFeature'`)
4. Push в branch (`git push origin feature/AmazingFeature`)
5. Откройте Pull Request

## 📝 Лицензия

MIT License - см. LICENSE файл

## 🙏 Благодарности

- WinMTR - оригинальная идея
- Tauri - отличная framework для desktop приложений
- React и Tailwind - лучшее комбо для UI

---

**Важно**: Для работы требуется запуск от имени Administrator (raw ICMP sockets).
