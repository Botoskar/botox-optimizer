# Propuesta de licencia (pendiente de aprobación del dueño)

El repo no tiene `LICENSE`. No se eligió una por vos: es una decisión tuya. Hasta que haya una, el código es "todos los derechos reservados" y nadie más puede usarlo, copiarlo ni redistribuirlo legalmente, aunque el repo sea público.

## Opciones

| Opción | Qué permite | Cuándo conviene |
|---|---|---|
| **Sin licencia + repo privado** (recomendada si no querés que otros usen el código) | Nada a terceros. Tus amigos solo reciben el `.exe`. | Si el objetivo es solo pasar el programa, no el código. |
| **Licencia propietaria corta** ("se permite usar el .exe gratis; prohibido redistribuirlo modificado ni venderlo") | Uso del binario, nada del código. | Si vas a distribuir el `.exe` a más gente y querés reglas claras. |
| **MIT** | Cualquiera usa, copia y modifica, incluso para vender, manteniendo tu aviso de copyright. | Si querés que sea open source sin vueltas. |
| **GPL-3.0** | Igual que MIT, pero quien distribuya una versión modificada tiene que publicar su código con la misma licencia. | Open source, pero que nadie lo cierre. |

Cualquiera de las cuatro es compatible con las dependencias actuales (todas permisivas; ver `THIRD-PARTY-NOTICES.md`).

## Cómo aplicarla

Cuando decidas, se crea `LICENSE` con el texto elegido, se agrega `license = "..."` en `rust/Cargo.toml` y se borra este archivo.
