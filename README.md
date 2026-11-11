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
cmd //c ".\\cargo.cmd test"                          # 40 tests
BX_ADMIN=1 cmd //c ".\\cargo.cmd build --release"    # versión para usar: pide administrador
```

Si la app está abierta, el `.exe` queda bloqueado: por eso se usa una copia en `rust/dist` (no se sube a git).

## Sacar una versión nueva

La app busca sola, al abrir, la última versión en `botox-optimizer-releases` y ofrece un botón "Actualizar". No lleva ninguna clave: el repo de versiones es público y solo trae el `.exe`; el código fuente está en este repositorio.

1. Subí la versión en `rust/Cargo.toml` y compilá la versión con admin (arriba).
2. Copiá el `.exe` como **`BotoxOptimizer.exe`** (sin espacios: la app busca ese nombre) y sacá su hash: `Get-FileHash BotoxOptimizer.exe`.
3. Firmá el `.exe` con Authenticode (`signtool sign /fd SHA256 ...`) y comprobá con `Get-AuthenticodeSignature .\BotoxOptimizer.exe` que diga `Valid`. La versión de release no instala un archivo sin firma válida; usá un certificado de una CA en la que confíen las PCs (o instalá la raíz en ellas).
4. En `botox-optimizer-releases` > Releases > Draft a new release:
   - Tag `vX.Y.Z` (igual a la versión de `Cargo.toml`).
   - En la descripción, una línea `SHA256: <hash>`. La app no instala si el hash no coincide (también se compara con el digest del asset que publica GitHub).
   - Adjuntá `BotoxOptimizer.exe` y publicá (no como pre-release: la app mira solo la última publicada).

Las versiones anteriores a 2.1.0 no saben actualizarse: hay que pasarles el `.exe` una vez.

## Notas de seguridad

- Los hijos elevados (PowerShell, `sc`, `powercfg`, `shutdown`, `rstrui`, `nvidia-smi`) se lanzan por ruta absoluta desde `System32` y con un entorno saneado: no se confía en `PATH`, `TEMP`, `SystemRoot` ni en el directorio actual.
- Los respaldos van a `%ProgramData%\BotoxOptimizer\backup` con ACL solo para SYSTEM/Administradores y se validan contra el catálogo antes de revertir (una entrada ajena se saltea). El respaldo se borra recién cuando la reversión terminó bien.
- `BX_SHOT`, `BX_PAGE`, `BX_TAB`, `BX_DEMO`, `BX_CONFIRM`, `BX_REBOOT`, `BX_LAPTOP`, `BX_UPDATE_API` y `BX_BACKUP_DIR` solo se leen en builds de desarrollo. El `.exe` de release los ignora.
- La actualización verifica el SHA256 en el mismo proceso que instala y, en release, exige una firma Authenticode válida.

## Agregar un tweak

1. Agregalo en `tweaks.js`.
2. `node rust/tools/export-catalog.js`.
3. Si usa una condición nueva en `rec` o `needs`, portala en `rust/src/catalog.rs` (`cond`).
4. `cargo.cmd test` y `node rust/tools/parity.js <ruta al .exe sin admin>` (compara los estados de Rust con los scripts originales, solo lectura).

## AMD (Radeon y Ryzen)

Sin verificar en una Radeon real (la PC de desarrollo es Ryzen + NVIDIA): lo que hay sale de documentación y de los tests.

- `amdtelemetry`: apaga los servicios `AUEPLauncher` y `AMD Crash Defender Service` y las tareas de Adrenalin. **No** toca `AMD External Events Utility` (lo usa FreeSync). Fuente: wiki de RadeonSoftwareSlimmer.
- HAGS se recomienda en NVIDIA y en Radeon RX 6000 o más nueva (`amdRdna2`, ver `is_rdna2` en `sys.rs`).
- Ryzen X3D de **doble chiplet** (7900X3D, 7950X3D, 9900X3D, 9950X3D; `x3dDual`: X3D con 12+ núcleos): no se ofrecen "Máximo rendimiento" ni "Desactivar estacionamiento de núcleos", porque el driver de V-Cache (`amd3dvcacheSvc`) usa el estacionamiento para mandar los juegos al chiplet con caché. El escáner agrega `x3d` (driver, Modo Juego, Game Bar) y la página Escáner muestra un hallazgo.

## Escáner de hardware

`rust/src/hwscan.rs` lee todo con WMI nativo (~0,3 s) y arma el mismo JSON que `scan.js`. Si WMI falla, la app cae solo al script de PowerShell (`scan.js`, ~5 s). Si agregás un dato al escáner, agregalo en los dos.

`node rust/tools/parity-scan.js [ruta al .exe sin admin]` compara los dos campo por campo (solo lectura). Con el `.exe` de admin: `exe --scan > nativo.json`, `exe --scan ps > ps.json` (elevado) y `node rust/tools/parity-scan.js --files nativo.json ps.json`.

## Probar con hardware AMD simulado

`BX_FAKE=amd` (Ryzen 7 5700 + RX 7600), `x3d` (Ryzen 9 7950X3D sin driver de V-Cache) o `x3d-ok` reemplazan CPU y GPU del escaneo para ver la app como la vería una PC AMD. Es solo lectura: en este modo ninguna acción cambia nada, aunque la app tenga administrador. En `rust/dist/prueba` hay una copia sin admin con un acceso `.cmd` por variante (usan una carpeta temporal para no tocar tus datos).

## Pruebas visuales

`rust/tools/shots.ps1 <carpeta>` saca una captura de cada pantalla sin cambiar nada en la PC.
