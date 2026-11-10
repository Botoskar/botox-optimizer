# Botox Optimizer

Optimizador de FPS y latencia para PC gamer con Windows 10/11. App nativa en Rust (egui), un solo `.exe` de 7 MB.

## Dónde está cada cosa

- `rust/`: la app. El código está en `rust/src`, y el `.exe` listo para usar en `rust/dist/Botox Optimizer.exe`.
- `tweaks.js` y `scan.js`: el catálogo de tweaks y el script del escáner. La app no los lee directo: `node rust/tools/export-catalog.js` los convierte en `rust/assets/catalog.json`.
- `_electron-viejo/`: la versión 1.1.0 en Electron, retirada. Se puede borrar cuando ya no haga falta.

## Compilar

El toolchain (Rust + mingw64) está en `M:\Github Things\botox-optimizer-rust` y no está en el PATH. `rust/cargo.cmd` lo configura:

```bash
cd rust
cmd //c ".\\cargo.cmd build"                         # desarrollo, sin admin (modo solo lectura)
cmd //c ".\\cargo.cmd test"                          # 11 tests
BX_ADMIN=1 cmd //c ".\\cargo.cmd build --release"    # versión para usar: pide administrador
```

Si la app está abierta, el `.exe` queda bloqueado: por eso se usa la copia de `rust/dist`.

## Agregar un tweak

1. Agregalo en `tweaks.js`.
2. `node rust/tools/export-catalog.js`.
3. Si usa una condición nueva en `rec` o `needs`, portala en `rust/src/catalog.rs` (`cond`).
4. `cargo.cmd test` y `node rust/tools/parity.js` (compara los estados de Rust con los scripts originales, solo lectura).

## Pruebas visuales

`rust/tools/shots.ps1 <carpeta>` saca una captura de cada pantalla sin cambiar nada en la PC.
