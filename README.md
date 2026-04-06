# Zona

Монорепозиторий: P2P-сеть серверов **zona-p2p** (Rust) + компаньон-сервер **Zona** (Rust) + библиотека **Zona.ProxyLib** для ASP.NET Core.

---

## zona-p2p — децентрализованная P2P-сеть

Реализация децентрализованной P2P-сети без центрального координатора: XOR-маршрутизация (Kademlia-like), Ed25519-идентификация узлов, Byzantine Fault Tolerance, Peer Exchange, Rate Limiting.

### Состав (`zona-p2p/`)

| Crate | Назначение |
|-------|------------|
| `zona-p2p-types` | Базовые типы: `NodeId`, `Descriptor`, `Envelope`, `PeerTable`, … |
| `zona-p2p-crypto` | Ed25519 keypair, NodeId = SHA-256(pubkey), подпись дескрипторов |
| `zona-p2p-overlay` | XOR-маршрутизация, PEX, Rate Limiting, DescriptorStore, Recovery |
| `zona-p2p-transport` | Framed TCP codec (`bincode` + length-prefix) |
| `zona-p2p-gossip` | Gossip-распространение дескрипторов |
| `zona-p2p-node` | Узел сети + HTTP admin API (axum) для Docker-режима |
| `zona-p2p-sim` | Детерминированный in-process симулятор + интерактивный REPL |

### Интерактивный симулятор (два режима)

```
InteractiveRepl
      │
 NetworkBackend (trait)
  ┌───┴───┐
  │       │
Sandbox  Docker
(in-proc) (HTTP → реальные контейнеры)
```

#### Sandbox-режим (по умолчанию)

Полностью in-process, без Docker. Симулирует сеть из N узлов, доставку сообщений, разделы и восстановление.

```powershell
.\scripts\interactive-p2p.ps1              # 8 узлов
.\scripts\interactive-p2p.ps1 -Nodes 12   # задать число
```

#### Docker-режим

Реальные Rust-узлы в контейнерах на текущей машине. Маршрутизация через реальный HTTP relay hop-by-hop.

```powershell
# 1. Поднять контейнеры (генерирует docker-compose.yml + запускает):
.\scripts\docker-p2p.ps1 -N 6

# 2. Подключиться интерактивным CLI:
.\scripts\interactive-p2p.ps1 -Docker -Nodes 6

# Остановить:
.\scripts\docker-p2p.ps1 -Down
```

#### Команды REPL

| Команда | Описание |
|---------|----------|
| `nodes` | список узлов (индекс, ID, слоты, статус) |
| `clients` | список клиентов и их узлов |
| `stats` | статистика сети |
| `peers <node>` | таблица маршрутизации узла |
| `send <from> <to> <msg>` | отправить сообщение между клиентами через сеть |
| `inbox <client>` | входящие сообщения клиента |
| `add` | добавить узел (sandbox) |
| `kill <node>` | убить узел |
| `partition [a b]` | разделить сеть на группы |
| `heal` | восстановить соединение после раздела |
| `repair` | принудительный repair-тик |
| `step [n]` | продвинуть симуляцию на n тиков |
| `drop [p]` | установить/показать вероятность потери пакета |

**Пример сессии:**
```
p2p> nodes
p2p> send c0 c4 "привет"
  Route: [0] → [2] → [4]
  ✓ DELIVERED  in 2 hop(s)

p2p> inbox c4
  #0  from=c0  msg: "привет"

p2p> kill 2
p2p> send c0 c4 "снова"
  ✗ NOT DELIVERED  (local minimum or dead route)

p2p> repair
p2p> send c0 c4 "снова"
  ✓ DELIVERED  in 3 hop(s)
```

### Сборка и проверка

Все команды Cargo запускаются через скрипты (обход ошибки MinGW с кириллическими путями):

```powershell
.\scripts\check-p2p.ps1          # cargo check --workspace
.\scripts\build-p2p.ps1          # cargo build --release
.\scripts\test-p2p.ps1           # cargo test --workspace
.\scripts\sim-p2p.ps1            # запустить автоматическую симуляцию
.\scripts\interactive-p2p.ps1    # интерактивный REPL (sandbox)
```

### Docker: admin HTTP API узла

Каждый контейнер слушает на порту `7701` (HTTP admin). Порты хоста: `17701`, `17702`, … (node0, node1, …).

| Endpoint | Метод | Назначение |
|----------|-------|------------|
| `/api/info` | GET | ID узла, здоровье, число слотов |
| `/api/peers` | GET | таблица маршрутизации |
| `/api/inbox` | GET | принятые сообщения |
| `/api/introduce` | POST | регистрация нового пира (bootstrap) |
| `/api/send` | POST | отправить сообщение (точка входа) |
| `/api/relay` | POST | hop-by-hop relay (внутренний) |

Переменные окружения контейнера:

| Переменная | Описание |
|------------|----------|
| `ZONA_NODE_SEED` | 64 hex-символа (32 байта) — детерминированный keypair |
| `ZONA_ADMIN_URL` | URL этого узла (e.g. `http://node0:7701`) |
| `ZONA_ADMIN_PORT` | порт admin API (по умолчанию `7701`) |
| `ZONA_BOOTSTRAP_PEERS` | через запятую admin URL-ы для начального bootstrap |

---

## Zona HTTP-сервер + Zona.ProxyLib

Компаньон-сервер **Zona** на Rust и библиотека **Zona.ProxyLib** для ASP.NET Core: приложение может автоматически поднимать процесс Zona, а на каждый входящий HTTP-запрос **асинхронно** (без блокировки Kestrel) отправлять уведомление на эндпоинт **`POST /teach`**.

### Состав

| Компонент | Путь | Назначение |
|-----------|------|------------|
| **Zona** (Rust) | `zona/` | HTTP-сервер (Axum): `lib` (`app`) + модуль teach; приём `POST /teach` |
| **Zona.ProxyLib** | `src/Zona.ProxyLib/` | DI, hosted service процесса, middleware + фоновая отправка teach |
| **Zona.Api** | `src/Zona.Api/` | Пример Web API с подключённой либой |

Решение Visual Studio / `dotnet`: `Zona.sln`.

### Архитектура

**Rust-крейт `zona`:** библиотека (`src/lib.rs`, публичный `zona::app()` и константа `TEACH_PATH`) собирает маршруты; обработчик teach вынесен в `src/teach.rs`; интеграционные тесты — в `tests/`; точка входа процесса — `src/main.rs` (инициализация логов, `ZONA_LISTEN`, `axum::serve`).

1. При старте приложения (если `Zona:AutoStartProcess` = `true`) **ZonaProcessHostedService** запускает бинарник Zona и передаёт слушать адрес через переменную окружения **`ZONA_LISTEN`** (значение из `Zona:Listen`). После старта ожидается успешное TCP-подключение к этому адресу (таймаут — `Zona:ProcessReadyTimeoutMs`).

2. **ZonaTeachMiddleware** стоит **первым** в пайплайне: для каждого запроса кладёт сигнал в очередь и **сразу** вызывает следующий middleware — обработка запроса не ждёт ответа от Zona.

3. **ZonaTeachDispatchWorker** (`BackgroundService`) забирает сигналы из канала и отправляет **`POST`** на `http://{Listen}{TeachPath}` с телом JSON в camelCase: `method`, `path`, `query`, `unixMs`.

4. На **Windows** дочерний процесс привязывается к **Job Object** с флагом *kill on job close*: при аварийном завершении процесса-хоста дескриптор job закрывается вместе с процессом, дочерний процесс завершается. При штатной остановке хоста вызывается **`Process.Kill(entireProcessTree: true)`**. На Linux полагаемся на явное завершение при остановке хоста.

### Требования

- [.NET 8 SDK](https://dotnet.microsoft.com/download/dotnet/8.0)
- [Rust](https://www.rust-lang.org/tools/install) (stable) и **один из линкеров для Windows** (см. ниже).

### Windows: ошибка `linker link.exe not found`

По умолчанию `rustup` ставит цель **`x86_64-pc-windows-msvc`**: для неё нужен линкер из **Visual Studio** или **Build Tools**.

### Вариант 1 (рекомендуется): MSVC

1. Установите [Build Tools for Visual Studio](https://visualstudio.microsoft.com/visual-cpp-build-tools/).
2. В установщике выберите рабочую нагрузку **«Разработка классических приложений на C++»** (Desktop development with C++), либо в «Отдельных компонентах» отметьте **MSVC**, **Windows SDK** и средства сборки C++.
3. Дождитесь окончания установки, **полностью закройте и снова откройте** терминал (лучше **Developer PowerShell for VS** или **x64 Native Tools Command Prompt** из меню «Пуск»).
4. Снова выполните: `cd zona` → `cargo build --release`.

Через [winget](https://learn.microsoft.com/windows/package-manager/winget/) (одна из типичных команд; при необходимости подправьте под свою версию):

```powershell
winget install Microsoft.VisualStudio.2022.BuildTools --silent --override `
  "--wait --quiet --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended"
```

После установки перезапустите терминал.

### Вариант 2: GNU (MinGW), без MSVC

1. Установите [MinGW-w64](https://www.mingw-w64.org/downloads/) так, чтобы **`gcc.exe`** был в `PATH` (часто ставят через [MSYS2](https://www.msys2.org/) или Chocolatey).
2. Установите и переключите toolchain под GNU:

```powershell
rustup toolchain install stable-x86_64-pc-windows-gnu
rustup default stable-x86_64-pc-windows-gnu
```

3. В каталоге `zona` снова: `cargo build --release`.

Бинарник будет собираться под `*-windows-gnu`; для **Zona.Api** копирование `zona.exe` в вывод по-прежнему подходит.

#### GNU: `dlltool.exe` / «cannot find» при сборке

Если при **`x86_64-pc-windows-gnu`** появляется **`error calling dlltool 'dlltool.exe': program not found`** или **`ld: cannot find ...`** для файлов в `target\debug\deps\`, часто виноваты одно или оба фактора:

1. **Неполный MinGW** — рядом с `gcc` должны быть **`dlltool.exe`**, **`ld.exe`** (полный toolchain). В [MSYS2](https://www.msys2.org/), 64-bit окружение MinGW:
   - `pacman -S mingw-w64-x86_64-toolchain`
   - В `PATH` для сборки из PowerShell обычно добавляют **`C:\msys64\mingw64\bin`** (путь проверьте у себя). Команда `where.exe dlltool` должна находить исполняемый файл.

2. **Путь проекта с кириллицей или длинный путь под OneDrive** (`...\Документы\...`) — старый линкер MinGW часто **некорректно открывает** такие пути. **Исправление в репозитории:** из корня выполните **`.\scripts\build-zona.ps1`** — скрипт задаёт `CARGO_TARGET_DIR` в `%LOCALAPPDATA%\Zona\cargo-target` (только ASCII) и копирует `zona.exe` в **`zona\target\release\`**, как ожидает .NET-проект. Дополнительно можно перейти на toolchain **MSVC** или перенести клон в `C:\dev\zona`.

### Сборка

#### Rust (Zona)

**Windows (в т.ч. путь с кириллицей / OneDrive):** из корня репозитория:

```powershell
.\scripts\build-zona.ps1
```

Отладочная сборка: `.\scripts\build-zona.ps1 -DebugBuild`.

**Linux / macOS:** скрипт тоже работает (каталог артефактов: `~/.cache/zona-cargo-target`). Либо классически:

```bash
cd zona
cargo build --release
```

Бинарник после скрипта или обычной сборки: `zona/target/release/zona.exe` (Windows) или `zona/target/release/zona` (Unix).

#### Тесты Rust (Zona)

Интеграционные тесты лежат в **`zona/tests/`** (отдельный таргет cargo; см. `teach.rs`).

На **Windows** с путём к проекту в кириллице / OneDrive запускайте тесты **тем же способом**, что и сборку: скрипт выставляет `CARGO_TARGET_DIR` и при наличии MSYS2 — дополняет `PATH` для `windows-gnu`.

```powershell
.\scripts\test-zona.ps1
```

Дополнительные аргументы передаются в `cargo test` (после `--` при необходимости):

```powershell
.\scripts\test-zona.ps1 -- --nocapture
```

**Linux / macOS:** скрипт `test-zona.ps1` задаёт тот же каталог артефактов, что `build-zona.ps1` (`~/.cache/zona-cargo-target`). Либо вручную:

```bash
export CARGO_TARGET_DIR="$HOME/.cache/zona-cargo-target"
mkdir -p "$CARGO_TARGET_DIR"
cd zona && cargo test
```

#### .NET

```bash
dotnet build Zona.sln -c Release
```

В **Zona.Api** при наличии файла `zona/target/release/zona.exe` он копируется в выходной каталог как `zona.exe`, что упрощает запуск с пустым `Zona:ExecutablePath`.

### Запуск примера API

Из каталога репозитория:

```bash
dotnet run --project src/Zona.Api --launch-profile http
```

Убедитесь, что бинарник Zona найден (сборка Rust + копирование в вывод **или** указан `Zona:ExecutablePath` / переменная **`ZONA_EXECUTABLE`**). Иначе при `AutoStartProcess: true` старт упадёт с сообщением о missing file.

Если Zona запускается вручную:

```bash
# Windows PowerShell
$env:ZONA_LISTEN = "127.0.0.1:8787"
.\zona\target\release\zona.exe
```

В `appsettings.json` выставьте `"AutoStartProcess": false`, чтобы API не пыталось само стартовать процесс.

### Конфигурация (`Zona`)

Секция в `appsettings.json` (или переменные окружения с префиксом `Zona__`):

| Ключ | Описание |
|------|----------|
| `AutoStartProcess` | Запускать ли процесс Zona вместе с приложением |
| `ExecutablePath` | Путь к бинарнику; пусто — `ZONA_EXECUTABLE`, затем `zona.exe` рядом с приложением |
| `Listen` | `host:port` для `ZONA_LISTEN` и для HTTP-клиента к teach |
| `TeachPath` | Путь на сервере Zona (по умолчанию `/teach`) |
| `ProcessReadyTimeoutMs` | Сколько ждать доступности TCP после старта процесса |

### Эндпоинт Zona: teach

- **Метод:** `POST`
- **Путь:** `/teach`
- **Тело (JSON):** `{ "method": "string", "path": "string", "query": "string|null", "unixMs": number }`
- **Ответ:** `204 No Content`

### Подключение либы в своём проекте

1. Добавьте ссылку на проект или пакет **Zona.ProxyLib**.

2. В `Program.cs`:

```csharp
builder.Services.AddZona();
// …
app.UseZonaTeach(); // первым среди middleware
```

3. Настройте секцию **`Zona`** в конфигурации и обеспечьте наличие бинарника Zona при `AutoStartProcess: true`.
