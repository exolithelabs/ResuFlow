# Third-party notices

ResuFlow includes third-party software. Their licenses remain separate
from the Apache-2.0 license for ResuFlow itself.

## Rust dependencies

The Rust + Tauri backend and desktop shell directly depend on:

- `tauri` — Apache-2.0 OR MIT
- `serde` — Apache-2.0 OR MIT
- `serde_json` — Apache-2.0 OR MIT
- `uuid` — Apache-2.0 OR MIT

## Build-time JavaScript tooling

During development and packaging, the frontend and desktop build use:

- `@tauri-apps/cli` — Apache-2.0 OR MIT
- `typescript` — Apache-2.0
- `vite` — MIT

The complete dependency manifests and lockfiles (`Cargo.toml`, `Cargo.lock`, `package.json`, `package-lock.json`) are included in the source tree.

Please consult each dependency's own license and notice files when redistributing a modified build.
