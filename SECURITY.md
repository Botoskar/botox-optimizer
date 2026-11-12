# Seguridad

## Versiones con soporte

Solo la última versión publicada en https://github.com/Botoskar/botox-optimizer-releases recibe correcciones. La app se actualiza sola; las anteriores a 2.1.0 no saben hacerlo.

## Cómo reportar una vulnerabilidad

**No abras un issue público.** Usá el reporte privado de GitHub: pestaña *Security* > *Report a vulnerability* en este repositorio (hay que activar "Private vulnerability reporting" en *Settings* > *Code security*).

Incluí qué versión, qué pasos lo reproducen y qué impacto tiene. Se responde en cuanto se pueda; es un proyecto personal sin plazos garantizados.

## Qué cuenta como vulnerabilidad

La app corre como administrador. Interesa especialmente:

- Cualquier forma de que un proceso **sin privilegios** consiga que la app ejecute código, escriba en el registro o borre archivos como administrador. Por ejemplo, con archivos plantados en `%ProgramData%\BotoxOptimizer`, variables de entorno, el directorio actual, respaldos manipulados o el `PATH`.
- Formas de que el actualizador instale un `.exe` que no sea el publicado y firmado.

No cuentan: que un administrador pueda dañar su propia PC, ni los tweaks funcionando como están documentados.
