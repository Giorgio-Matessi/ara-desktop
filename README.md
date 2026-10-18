# ARA Desktop

Desktop app built with [Tauri 2](https://tauri.app), React and TypeScript (Vite).

## Prerequisites

- [Node.js](https://nodejs.org) 20+
- [Rust](https://www.rust-lang.org/tools/install) (stable)
- Tauri system dependencies: see https://tauri.app/start/prerequisites/

## Development

```sh
npm install
npm run tauri dev
```

## Build

```sh
npm run tauri build
```

Installers are written to `src-tauri/target/release/bundle/`.
