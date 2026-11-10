# Botox Optimizer

Optimizador de FPS y latencia para PC gamer con Windows 10/11. App nativa en Rust (egui), un solo `.exe` de 10 MB.

## Dónde está cada cosa

- `rust/`: la app. El código está en `rust/src`.
- `tweaks.js` y `scan.js`: el catálogo de tweaks y el script del escáner. La app no los lee directo: `node rust/tools/export-catalog.js` los convierte en `rust/assets/catalog.json`.
- Las versiones listas para usar se publican en un repo público aparte, solo con el `.exe`: https://github.com/Botoskar/botox-optimizer-releases

## Compilar

El toolchain (Rust + mingw64) está en `M:\Github Things\botox-optimizer-rust` y no está en el PATH. `rust/cargo.cmd` lo configura:

```bash
cd rust
cmd //c ".\\cargo.cmd build"                         # desarrollo, sin admin (modo solo lectura)
cmd //c ".\\cargo.cmd test"                          # 24 tests
BX_ADMIN=1 cmd //c ".\\cargo.cmd build --release"    # versión para usar: pide administrador
```

Si la app está abierta, el `.exe` queda bloqueado: por eso se usa una copia en `rust/dist` (no se sube a git).

## Sacar una versión nueva

La app busca sola, al abrir, la última versión en `botox-optimizer-releases` y ofrece un botón "Actualizar". No lleva ninguna clave: el repo de versiones es público y el código de este repo es privado.

1. Subí la versión en `rust/Cargo.toml` y compilá la versión con admin (arriba).
2. Copiá el `.exe` como **`BotoxOptimizer.exe`** (sin espacios: la app busca ese nombre) y sacá su hash: `Get-FileHash BotoxOptimizer.exe`.
3. En `botox-optimizer-releases` > Releases > Draft a new release:
   - Tag `vX.Y.Z` (igual a la versión de `Cargo.toml`).
   - En la descripción, una línea `SHA256: <hash>`. La app no instala si el hash no coincide.
   - Adjuntá `BotoxOptimizer.exe` y publicá (no como pre-release: la app mira solo la última publicada).

Las versiones anteriores a 2.1.0 no saben actualizarse: hay que pasarles el `.exe` una vez.

## Agregar un tweak

1. Agregalo en `tweaks.js`.
2. `node rust/tools/export-catalog.js`.
3. Si usa una condición nueva en `rec` o `needs`, portala en `rust/src/catalog.rs` (`cond`).
4. `cargo.cmd test` y `node rust/tools/parity.js <ruta al .exe sin admin>` (compara los estados de Rust con los scripts originales, solo lectura).

## Escáner de hardware

`rust/src/hwscan.rs` lee todo con WMI nativo (~0,3 s) y arma el mismo JSON que `scan.js`. Si WMI falla, la app cae solo al script de PowerShell (`scan.js`, ~5 s). Si agregás un dato al escáner, agregalo en los dos.

`node rust/tools/parity-scan.js [ruta al .exe sin admin]` compara los dos campo por campo (solo lectura). Con el `.exe` de admin: `exe --scan > nativo.json`, `exe --scan ps > ps.json` (elevado) y `node rust/tools/parity-scan.js --files nativo.json ps.json`.

## Pruebas visuales

`rust/tools/shots.ps1 <carpeta>` saca una captura de cada pantalla sin cambiar nada en la PC.
