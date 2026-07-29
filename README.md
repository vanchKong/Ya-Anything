# YaYa (压鸭)

Local-first desktop tool for compressing images and GLB/glTF 3D models. Files are overwritten in place with session undo support — nothing is uploaded.

[中文文档](./README-zh.md)

## Features

### Images
- **Compress** — re-encode by quality only; **never changes width/height**
- **Resize** — width / height (aspect locked by default) or percentage, then encode at the same quality
- Dual drop zones for the two workflows; batch-friendly shared rules
- JPG / PNG / GIF / WebP (animated formats: compress yes, resize no for multi-frame)

### Models (GLB)
- Presets: Extreme small / Compact / Balanced / Fast fidelity (+ Custom)
- Mesh: Draco / Meshopt / none, cleanup (dedup, weld, prune, …)
- Textures: format, max edge, quality, fast (canvas) vs strong (libwebp) encoder
- Preview, side-by-side compare with linked orbit controls

### App
- Tauri 2 + Vue 3, fully offline
- Theme: system / light / dark
- Progress, multi-select, restore originals within the session

## Requirements

- [Node.js](https://nodejs.org/) LTS
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/) (stable) for Tauri builds

## Develop

```bash
pnpm install
pnpm run dev:tauri
```

Frontend only (no native shell):

```bash
pnpm run dev
```

## Build locally

```bash
pnpm run build:tauri
```

Artifacts land under `src-tauri/target/release/bundle/`.

## Releases (CI)

On every push to `main` / `master`, GitHub Actions builds and publishes a **prerelease** with:

| Target | Runner / flags |
|--------|----------------|
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Windows x64 | `windows-latest` |

Workflow: [`.github/workflows/release.yml`](./.github/workflows/release.yml). You can also run it manually via **Actions → Release → Run workflow**.

> Grant the repo **Settings → Actions → General → Workflow permissions → Read and write permissions** so releases can be created.

## Stack

- UI: Vue 3, TypeScript, Vite
- Shell: Tauri 2 (Rust)
- Images: `image` / oxipng / libwebp / gif
- Models: glTF-Transform, Draco, Meshopt, Three.js preview

## License

[Apache License 2.0](./LICENSE)  
Copyright 2026 vanch

