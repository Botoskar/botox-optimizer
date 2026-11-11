# Botox Optimizer

Optimizador de FPS y latencia para PC gamer con Windows 10/11. App nativa en Rust (egui), un solo `.exe` de 10 MB.

## Dónde está cada cosa

- `rust/`: la app. El código está en `rust/src`.
- `tweaks.js` y `scan.js`: el catálogo de tweaks y el script del escáner. La app no los lee directo: `node rust/tools/export-catalog.js` los convierte en `rust/assets/catalog.json`.
- Las versiones listas para usar se publican en un repo público aparte, solo con el `.exe`: https://github.com/Botoskar/botox-optimizer-releases
- **Visibilidad:** este repositorio de código (`Botoskar/botox-optimizer`) es **público a propósito**. No tiene claves ni secretos, y no tiene que tenerlos: la clave privada de firma nunca se sube (ver "Sacar una versión nueva"). Sin `LICENSE`, que el código se vea no da permiso para reutilizarlo; ver `LICENSE-PROPUESTA.md`.
- `THIRD-PARTY-NOTICES.md`: licencias de lo que va dentro del `.exe` (Poppins bajo SIL OFL 1.1 y las dependencias). `SECURITY.md`: cómo reportar vulnerabilidades. `docs/DISENO-MOTOR-ELEVADO.md`: propuesta para separar la interfaz del motor elevado.

## Si algo sale mal

- **Revertir todo:** en Backups, o con **Ctrl+Shift+R** desde cualquier sección. Pide confirmación antes de tocar nada.
- **Modo seguro:** `"Botox Optimizer.exe" --safe` abre la app mostrando solo Backups. Ahí se puede revertir, crear un punto de restauración y reiniciar, pero no aplicar nada. En Backups hay un botón para copiar el comando.
- **Sin ventana:** `"Botox Optimizer.exe" --revert-all` muestra qué revertiría, sin tocar nada. `--revert-all --yes` lo revierte y avisa el resultado en una ventanita. No corre si la app está abierta ni en modo prueba.

## Compilar

El toolchain (Rust + mingw64) está en `M:\Github Things\botox-optimizer-rust` y no está en el PATH. `rust/cargo.cmd` lo configura:

```bash
cd rust
cmd //c ".\\cargo.cmd build"                         # desarrollo, sin admin (modo solo lectura)
cmd //c ".\\cargo.cmd test"                          # 49 tests (nunca los corras elevado)
BX_ADMIN=1 cmd //c ".\\cargo.cmd build --release"    # versión para usar: pide administrador
powershell -NoProfile -File tools\parse-ps.ps1       # sintaxis de los 113 scripts del catálogo, sin ejecutarlos
```

Si la app está abierta, el `.exe` queda bloqueado: por eso se usa una copia en `rust/dist` (no se sube a git).

**Versión de Rust:** 1.99.1, variante GNU (`x86_64-pc-windows-gnu`). Queda fijada en `rust/Cargo.toml` (`rust-version`) y en CI. No hay `rust-toolchain.toml` a propósito: con rustup, un `1.99.1` fijo es otro toolchain distinto del `stable` que ya tenés. rustup lo bajaría de nuevo, o el build fallaría sin red. Si querés fijarlo igual: `rustup toolchain install 1.99.1-x86_64-pc-windows-gnu` y después agregás el archivo con `channel = "1.99.1"`.

**Build reproducible sin `M:` ni `cargo.cmd`:** el workflow `.github/workflows/ci.yml` compila en una máquina limpia de GitHub con Rust 1.99.1 GNU y MinGW de MSYS2. Ese workflow es la receta de referencia. `rustfmt` y `clippy` no están instalados en el toolchain local; en CI corren como informativos.

## Sacar una versión nueva

La app busca sola, al abrir, la última versión en `botox-optimizer-releases` y ofrece un botón "Actualizar". No lleva ninguna clave: el repo de versiones es público y solo trae el `.exe`; el código fuente está en este repositorio.

### Certificado de firma (autofirmado, gratis)

El `.exe` de release solo instala actualizaciones firmadas con Authenticode **por el certificado fijado** en `PUBLISHER_THUMBPRINTS` (`rust/src/update.rs`).
- El certificado es autofirmado: `CN=Botox Optimizer`, thumbprint `2DBA1019FCB52F90A5692BA2F4FACF02976DABD7`, vence el 2046-11-11.
- Lo creó `rust/tools/new-signing-cert.ps1` en `Cert:\CurrentUser\My` de la PC del dueño. **La clave privada no está en el repo y nunca se sube.**
- Windows no confía en su raíz, así que el estado aparece como `UnknownError`. Eso es esperado: la app acepta una raíz no confiable **solo** si el firmante es el thumbprint fijado. Un archivo adulterado falla igual, con otro error.
- Lo que no cambia: Windows sigue mostrando "Editor desconocido" en el aviso de UAC y SmartScreen puede advertir. El certificado protege las actualizaciones, no la primera descarga.

**Hacé una copia de la clave fuera de esta PC.** Si se pierde (formateo, disco roto), ninguna versión publicada va a poder actualizarse sola, y habrá que pasar el `.exe` a mano. Para exportarla, con una contraseña que elegís vos:
```powershell
$c = Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object Subject -eq 'CN=Botox Optimizer'
Export-PfxCertificate -Cert $c -FilePath D:\backup\botox-firma.pfx -Password (Read-Host -AsSecureString 'Contraseña')
```
Guardá el `.pfx` en un pendrive o en una nube personal, nunca en este repo. Para restaurarla en otra PC: `Import-PfxCertificate -FilePath botox-firma.pfx -CertStoreLocation Cert:\CurrentUser\My -Exportable -Password (Read-Host -AsSecureString)`.

Para cambiar de certificado: agregá el nuevo thumbprint a la lista en una versión firmada con el viejo, y recién en la siguiente firmá con el nuevo. Cada versión verifica a la que viene después.

### Pasos

1. Subí la versión en `rust/Cargo.toml` y compilá la versión con admin (arriba). Usá siempre tres números, `X.Y.Z`: una versión de cuatro (`1.2.3.4`) no se reconoce y la actualización no se ofrece.
2. Copiá el `.exe` como **`BotoxOptimizer.exe`** (sin espacios: la app busca ese nombre).
3. Firmalo: `powershell -NoProfile -File rust\tools\sign.ps1 <ruta>\BotoxOptimizer.exe`. Firma con timestamp, comprueba que el firmante sea el certificado fijado y muestra el `SHA256` (sacado después de firmar, porque la firma cambia el archivo).
4. En `botox-optimizer-releases` > Releases > Draft a new release:
   - Tag `vX.Y.Z` (igual a la versión de `Cargo.toml`).
   - En la descripción, una línea `SHA256: <hash>`. La app no instala si el hash no coincide (también se compara con el digest del asset que publica GitHub).
   - Adjuntá `BotoxOptimizer.exe` y publicá (no como pre-release: la app mira solo la última publicada).

Las versiones anteriores a 2.1.0 no saben actualizarse: hay que pasarles el `.exe` una vez. Las de 2.1.0 a 2.3.1 instalan la siguiente sin mirar la firma; la verificación de firma y pin empieza a regir en la versión que la incluye.

### Qué pasa al actualizar (y cómo vuelve atrás)

1. Se baja `BotoxOptimizer.nuevo.exe`. Se verifican tamaño, `MZ` y SHA256 en el mismo proceso, y se escribe la copia `BotoxOptimizer.verified.exe`.
2. Sobre esa copia, la que se instala, se verifican la firma y el thumbprint fijado. Después se corre `BotoxOptimizer.verified.exe --version`: tiene que contestar la versión esperada en menos de 20 s.
3. Se vuelve a calcular el hash, el `.exe` actual pasa a `BotoxOptimizer.old.exe` y la copia verificada toma su lugar. Queda el marcador `BotoxOptimizer.update-pending` con `0`.
4. La versión nueva, al abrir, pasa el marcador a `1`. Cuando dibuja su primer cuadro, borra el marcador y `.old.exe`.
5. Si se abre y encuentra `1`, la vez anterior no llegó a dibujar. Entonces pasa a `BotoxOptimizer.failed.exe`, restaura `.old.exe`, la abre y sale.

Esto protege solo las actualizaciones hechas **desde** una versión que ya trae este código.

## Notas de seguridad

- Los hijos elevados (PowerShell, `sc`, `powercfg`, `shutdown`, `rstrui`, `nvidia-smi`) se lanzan por ruta absoluta desde `System32` y con un entorno saneado: no se confía en `PATH`, `TEMP`, `SystemRoot` ni en el directorio actual.
- `winget` no se busca en el `PATH`: se resuelve en `Program Files\WindowsApps`, que está protegida. El alias de `%LOCALAPPDATA%\Microsoft\WindowsApps` es del usuario, igual que `OneDriveSetup.exe` del perfil, que solo se ejecuta si lo firmó Microsoft.
- Los respaldos van a `%ProgramData%\BotoxOptimizer\backup` con ACL solo para SYSTEM/Administradores y se validan contra el catálogo antes de revertir (una entrada ajena se saltea). El respaldo se borra recién cuando la reversión terminó bien.
- ProgramData deja que un usuario normal cree carpetas y archivos, y el dueño de una carpeta siempre puede volver a abrir sus permisos (`icacls /grant:r` no cambia el dueño). Por eso, con la app elevada:
  - `%ProgramData%\BotoxOptimizer`, `tmp` (donde van los `.ps1`), `backup` y cada respaldo tienen que ser de SYSTEM o Administradores.
  - Si no lo son, no se usan y la acción falla sin tocar nada.
  - Pasa en Rust (`win::require_admin_owned`) y en PowerShell (`BkGuard`).
- `BX_SHOT`, `BX_PAGE`, `BX_TAB`, `BX_DEMO`, `BX_CONFIRM`, `BX_REBOOT`, `BX_LAPTOP`, `BX_UPDATE_API` y `BX_BACKUP_DIR` solo se leen en builds de desarrollo. El `.exe` de release los ignora. Excepción: `BX_FAKE` también se lee en release, porque los lanzadores de prueba usan ese `.exe`. Solo quita poder: con `BX_FAKE` la app bloquea toda acción. Ningún `BX_*` llega a los hijos elevados.
- La actualización verifica el SHA256 en el mismo proceso que instala. En release exige una firma Authenticode íntegra hecha por el certificado fijado (autofirmado; ver "Sacar una versión nueva"). Una raíz no confiable se acepta solo si el firmante es ese certificado. No hay variable de entorno para saltearlo.
- Una sola instancia (mutex `Local\BotoxOptimizer.Instancia`): otra ventana trae la existente al frente. Las copias con `BX_FAKE` pueden abrirse al lado de la real.
- Si la app se cierra por un error interno, deja `%LOCALAPPDATA%\BotoxOptimizer\crash.log`: fecha, versión, mensaje y backtrace, sin datos personales. Pasado 1 MB empieza de nuevo. `state.json` se guarda de forma atómica (temporal + rename).
- **UAC con otra cuenta** (el aviso lo aprueba un administrador distinto del usuario que abrió la app): la app elevada corre con el perfil de **ese administrador**. Las limpiezas de temporales, crash dumps y recientes, el avatar, `state.json` y `crash.log` usan su perfil, no el del usuario. No se intenta adivinar el perfil del otro usuario, porque no hay una forma confiable sin el refactor de `docs/DISENO-MOTOR-ELEVADO.md`. En la PC típica (usuario administrador) no pasa.
- Riesgo residual del actualizador: entre la verificación final y el rename, alguien con permiso de escritura en la carpeta del `.exe` podría cambiar la copia verificada. Ese alguien ya puede reemplazar el `.exe` directamente, así que no se agrega más complejidad.

## Marca

"Botox" es una marca registrada de AbbVie (el medicamento). Usarla como nombre de un software que se distribuye puede traer un reclamo de marca, aunque no tenga relación. Para un uso personal o entre amigos el riesgo es bajo. Si el proyecto crece o se publica más ampliamente, conviene consultarlo o cambiar el nombre. No se cambió nada.

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

`rust/tools/shots.ps1 <carpeta>` saca una captura de cada pantalla sin cambiar nada en la PC. Usa el build **debug**: el de release ignora `BX_SHOT` y `BX_PAGE`.

Lo mismo para probar el actualizador con un servidor local (`BX_UPDATE_API=http://127.0.0.1:…`): solo funciona en debug, y ahí no se exige firma.
