# Comprueba la sintaxis de todos los scripts de PowerShell del catálogo SIN ejecutarlos.
# Uso (con Windows PowerShell 5.1, no pwsh):  powershell -NoProfile -File rust\tools\parse-ps.ps1
# Sale con código 1 si alguno no parsea.
$ErrorActionPreference = 'Stop'
$cat = Get-Content -Raw -Encoding UTF8 (Join-Path $PSScriptRoot '..\assets\catalog.json') | ConvertFrom-Json

$scripts = [ordered]@{ 'prelude' = $cat.prelude; 'scan' = $cat.scan }
foreach ($t in $cat.tweaks) {
  if ($t.ps) { foreach ($k in 'apply', 'revert', 'check') { $scripts["$($t.id).$k"] = $t.ps.$k } }
}
foreach ($b in $cat.boosts) { $scripts["boost.$($b.id)"] = $b.ps }

$bad = 0
foreach ($name in $scripts.Keys) {
  # Mismo envoltorio que engine::run_ps: preludio + try/catch alrededor del script.
  $body = if ($name -eq 'prelude' -or $name -eq 'scan') { $scripts[$name] } else { "$($cat.prelude)`ntry {`n$($scripts[$name])`n} catch { exit 1 }" }
  $tokens = $null; $errors = $null
  [void][System.Management.Automation.Language.Parser]::ParseInput($body, [ref]$tokens, [ref]$errors)
  if ($errors.Count) {
    $bad++
    foreach ($e in $errors) { Write-Output "ERROR $name línea $($e.Extent.StartLineNumber): $($e.Message)" }
  }
}
Write-Output "PowerShell $($PSVersionTable.PSVersion): $($scripts.Count) scripts, $bad con errores"
if ($bad) { exit 1 }
