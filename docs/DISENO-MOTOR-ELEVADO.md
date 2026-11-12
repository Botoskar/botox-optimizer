# Diseño: interfaz sin admin + motor elevado

Estado: **propuesta, no implementada.** Cubre el punto 4 de `PENDIENTES.md`.

## Problema

Hoy todo el `.exe` corre elevado (`requireAdministrator`): la interfaz de egui, los decodificadores de imágenes (`image`, `tiff`, `png`), el portapapeles, la red del actualizador y el render. Un bug en cualquiera de esas partes tiene impacto de administrador. Lo que de verdad necesita admin es poco y está bien delimitado.

## Qué necesita elevación

| Operación | Necesita admin | Hoy en |
|---|---|---|
| Aplicar o revertir tweaks (registro HKLM, `sc.exe`, `powercfg`, scripts del catálogo) | Sí | `engine::toggle`, `revert_native`, `run_ps_with` |
| Boosts (limpiezas, DISM) | Sí, la mayoría | `engine::boost` |
| Crear un punto de restauración | Sí | `engine::create_restore_point` |
| Encender o apagar programas de inicio en HKLM | Sí (HKCU no) | `startup::set` |
| Leer respaldos y escribir en `%ProgramData%\BotoxOptimizer` | Sí | `reg::*backup*` |
| Instalar una actualización en una carpeta protegida | Depende de dónde esté el `.exe` | `update::install` |
| Escanear hardware, chequear estados, estadísticas, avatar, buscar actualizaciones | **No** | `hwscan`, `check_all_*`, `sys::stats`, `update::check` |

## Procesos

1. **`BotoxOptimizer.exe`** (manifiesto `asInvoker`): la interfaz completa. Escanea, chequea y muestra todo, como el modo "solo lectura" que ya existe.
2. **`BotoxOptimizer.exe --engine <pipe>`**: el mismo binario, lanzado con `ShellExecuteExW` + verbo `runas` (un solo aviso de UAC) la primera vez que el usuario pide una acción. Sin ventana. Muere solo si la interfaz se cierra o después de N minutos sin pedidos.

Mismo binario a propósito: no hay que instalar un servicio ni un segundo `.exe`, y el actualizador sigue reemplazando un solo archivo.

## Canal (IPC)

- **Named pipe** `\\.\pipe\BotoxOptimizer-<GUID aleatorio por sesión>`. El nombre se pasa por argumento al motor.
- El **motor crea el pipe** con un descriptor de seguridad que solo deja conectarse al SID del usuario interactivo (el que lanzó la interfaz) y a Administradores. `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE` y una sola instancia.
- Al conectar, el motor verifica con `GetNamedPipeClientProcessId` que el cliente sea el PID padre que lo lanzó (pasado por argumento) y que la imagen de ese proceso sea la misma ruta que la del motor.
- Mensajes: JSON delimitado por longitud (u32 LE + bytes), máximo 64 KB. Lo que no parsea o excede el tamaño corta la conexión.

## Mensajes (lista cerrada)

```text
Toggle    { id: String, on: bool }       // id debe existir en el catálogo incrustado
Boost     { id: String }                 // id debe existir en BOOSTS
RevertAll {}
CreateRestorePoint {}
StartupSet { key: String, on: bool }     // key debe aparecer en startup::scan() del propio motor
InstallUpdate {}                         // el motor vuelve a consultar GitHub; no recibe URL ni hash
Cancel {}
```

Reglas de validación:

- **El motor nunca recibe rutas, scripts, valores de registro, URLs ni hashes.** Solo ids, que busca en **su propio** catálogo incrustado (`Catalog::load()`). Lo que la interfaz no puede pedir, el motor no lo puede hacer.
- `InstallUpdate` no confía en lo que vio la interfaz: el motor repite `update::check()` y todo el flujo de hash, firma y pin.
- Respuestas: las líneas de log (`Line`) en streaming y un `Done { ok, err }` final, como hoy por `mpsc`.

## Cambios en el código

- `engine::Log` ya es un `Sender`: del lado del motor se reemplaza por un escritor del pipe, y del lado de la interfaz por un hilo que lee el pipe y reenvía por el `mpsc` actual. `App::act` casi no cambia: en vez de `self.spawn(engine::toggle…)`, manda el mensaje.
- `win::secure_command`, `script_dir`, respaldos y el chequeo de dueños quedan del lado del motor, sin cambios.
- La interfaz deja de necesitar `secure_command` para lo que es de lectura, aunque puede seguir usándolo.

## Compatibilidad

- Mientras se implementa, se mantiene el modo actual: si el `.exe` arranca ya elevado (por ejemplo, un acceso directo con "Ejecutar como administrador"), hace todo en proceso, como hoy.
- El manifiesto pasa a `asInvoker` **recién** cuando el motor esté probado. Hasta entonces `BX_ADMIN=1` sigue igual.

## Riesgos

- **Superficie nueva:** el pipe. Mitigación: lista cerrada de mensajes, ACL del pipe, chequeo del PID y de la imagen del cliente.
- **Over-the-shoulder** (el que aprueba el UAC es otro usuario): el SID del cliente ya no coincide con el del motor. Hay que pasar el SID del usuario interactivo por argumento y usarlo en la ACL del pipe.
- **UX:** el UAC aparece al primer clic de acción, no al abrir. Es lo esperable.

## Riesgo residual mientras no se implemente

Todo el proceso elevado procesa:

- imágenes elegidas por el usuario (mitigado en parte: límites de 50 MB y 12000 px);
- el portapapeles;
- la respuesta JSON de GitHub (la parsea serde, sin `unsafe`).

Un bug de memoria en `image` o `tiff` sería explotable con privilegios de administrador. Para reducirlo hoy sin el refactor: `cargo audit` en CI (ya está) y, si se quiere, sacar `tiff` de las features de `image`.

## Estimación

Unas 600 a 900 líneas (pipe con ACL en FFI, protocolo, ciclo de vida del motor, tests) más pruebas manuales en una VM. Conviene hacerlo en una rama aparte y no mezclarlo con otros cambios.
