# 压鸭（YaYa）

本地优先的桌面工具：压缩图片与 GLB/glTF 模型。结果写回原路径，支持会话内回退，**不会上传任何文件**。

[English](./README.md)

## 功能

### 图片
- **压缩**：只调质量，**绝不改变宽高**
- **改尺寸**：按宽 / 按高（默认锁比例）或百分比，再按同一「质量」编码
- 双投放区对应两种流程；批量使用同一套规则
- 支持 JPG / PNG / GIF / WebP（动图可压缩；多帧动图暂不支持改尺寸）

### 模型（GLB）
- 预设：极致小体积 / 轻量 / 均衡 / 快速保真（+ 自定义）
- 网格：Draco / Meshopt / 无，以及去重、焊接、清除冗余等
- 贴图：格式、边长、质量；快速（浏览器）与强力（libwebp）编码
- 预览、左右对比，视角联动

### 应用
- Tauri 2 + Vue 3，完全离线
- 主题：跟随系统 / 浅色 / 深色
- 进度、多选、本会话回退原文件

## 环境要求

- [Node.js](https://nodejs.org/) LTS
- [pnpm](https://pnpm.io/)
- [Rust](https://www.rust-lang.org/)（stable），用于 Tauri 构建

## 开发

```bash
pnpm install
pnpm run dev:tauri
```

仅前端（无原生壳）：

```bash
pnpm run dev
```

## 本地打包

```bash
pnpm run build:tauri
```

产物在 `src-tauri/target/release/bundle/`。

## 自动 Release（CI）

每次推送到 `main` / `master` 时，GitHub Actions 会构建并发布一个 **prerelease**，包含：

| 目标 | 说明 |
|------|------|
| macOS Apple Silicon | `aarch64-apple-darwin` |
| macOS Intel | `x86_64-apple-darwin` |
| Windows x64 | `windows-latest` |

工作流文件：[`.github/workflows/release.yml`](./.github/workflows/release.yml)。也可在 **Actions → Release → Run workflow** 手动触发。

> 请在仓库 **Settings → Actions → General → Workflow permissions** 勾选 **Read and write permissions**，否则无法创建 Release。

## 技术栈

- 界面：Vue 3、TypeScript、Vite
- 壳：Tauri 2（Rust）
- 图片：`image` / oxipng / libwebp / gif
- 模型：glTF-Transform、Draco、Meshopt、Three.js 预览

## 许可

[Apache License 2.0](./LICENSE)  
Copyright 2026 vanch

