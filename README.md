# Photo Curator

A Windows desktop app for quickly curating large photo collections through lightweight, repeated human decisions.

## Stack

- Tauri 2 / Rust
- Nuxt 3 / Vue 3 / TypeScript
- Vuetify 3
- Pinia
- Vitest

## Development

```powershell
pnpm install
pnpm tauri:dev
```

Useful checks:

```powershell
pnpm typecheck
pnpm test
pnpm build
cargo check --manifest-path src-tauri/Cargo.toml
```

The product workflow and screen structure will be designed before feature implementation begins.
