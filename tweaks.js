// Catálogo de optimizaciones. Cada tweak se convierte en PowerShell (aplicar / revertir / chequear).
// Los valores de registro se respaldan antes de cambiarlos en %ProgramData%\BotoxOptimizer\backup\<id>.json,
// así "revertir" deja exactamente lo que había antes.

// Ruta con "/" para que sea legible; se convierte a "\".
const R = (path, name, value, type = 'DWord', def = null) => [path.replace(/\//g, '\\'), name, value, type, def];
// Servicio: se toca su valor Start en el registro (2 auto, 3 manual, 4 deshabilitado). Solo si el servicio existe.
const S = (name, start, def) => ['HKLM:\\SYSTEM\\CurrentControlSet\\Services\\' + name, 'Start', start, 'DWord', def, 1];

const psq = v => v === null ? '$null'
  : Array.isArray(v) ? `([byte[]](${v.join(',')}))`
  : typeof v === 'number' ? String(v)
  : `'${String(v).replace(/'/g, "''")}'`;
const psList = es => es.length === 0 ? '@()'
  : es.length === 1 ? `@(,@(${es[0].map(psq).join(',')}))`
  : `@(${es.map(e => `@(${e.map(psq).join(',')})`).join(',')})`;
const psArr = a => '@(' + a.map(psq).join(',') + ')';

const PRELUDE = String.raw`
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$BK = [Environment]::GetFolderPath('CommonApplicationData') + '\BotoxOptimizer\backup'
$script:BKREADY = $false

# Consola de actividad: cada línea "LOG:nivel|mensaje" se muestra en vivo en la app.
# Va directo a la salida estándar, sin pasar por el pipeline, para no mezclarse con lo que devuelven las funciones.
function Log($m, $l = 'info') { [Console]::Out.WriteLine("LOG:$l|$m") }
function Short($p) { $p -replace '^HKLM:\\', 'HKLM\' -replace '^HKCU:\\', 'HKCU\' }
function Fmt($c) { if (!$c.e) { '(no existía)' } else { "$($c.v)" } }

function Get-RegVal($p, $n) {
  if (Test-Path -LiteralPath $p) {
    $k = Get-Item -LiteralPath $p
    if ($k.GetValueNames() -contains $n) { return @{ e = $true; v = $k.GetValue($n, $null, 'DoNotExpandEnvironmentNames'); t = [string]$k.GetValueKind($n) } }
  }
  @{ e = $false }
}
function Set-RegVal($p, $n, $v, $t) {
  # Arranque de servicios: vía el Administrador de servicios (SYSTEM), porque algunas claves solo permiten lectura a Administradores.
  if ($n -eq 'Start' -and $p -match '\\CurrentControlSet\\Services\\([^\\]+)$' -and 2..4 -contains [int]$v) {
    $s = $Matches[1]; $new = (@{ 2 = 'Automatic'; 3 = 'Manual'; 4 = 'Disabled' })[[int]$v]; $old = (Get-Service -Name $s).StartType
    Set-Service -Name $s -StartupType $new; Log "Servicio $($s): $old → $new"; return
  }
  $o = Get-RegVal $p $n
  if (!(Test-Path -LiteralPath $p)) { New-Item -Path $p -Force | Out-Null }
  if ($t -eq 'Binary') { $v = [byte[]]$v }
  New-ItemProperty -LiteralPath $p -Name $n -Value $v -PropertyType $t -Force | Out-Null
  Log "$(Short $p)\$($n): $(Fmt $o) → $v"
}
function Del-RegVal($p, $n) {
  $o = Get-RegVal $p $n
  if ($o.e) { Remove-ItemProperty -LiteralPath $p -Name $n -ErrorAction SilentlyContinue; Log "$(Short $p)\$($n): $(Fmt $o) → (eliminado)" }
}
# winget por ruta absoluta dentro de Program Files\WindowsApps (protegida). El alias de %LOCALAPPDATA%\Microsoft\WindowsApps
# está en una carpeta del usuario: un winget.exe falso ahí correría como administrador.
function WingetExe {
  $w = Get-AppxPackage Microsoft.DesktopAppInstaller -ErrorAction SilentlyContinue | Sort-Object Version -Descending |
    ForEach-Object { Join-Path $_.InstallLocation 'winget.exe' } | Where-Object { Test-Path -LiteralPath $_ } | Select-Object -First 1
  if (!$w) { throw 'No encontré winget (Instalador de aplicaciones de Microsoft). Instalá OneDrive desde la Microsoft Store.' }
  $w
}
function MsSigned($f) { $s = Get-AuthenticodeSignature -LiteralPath $f; $s.Status -eq 'Valid' -and $s.SignerCertificate.Subject -match 'O=Microsoft Corporation' }
function StopSvc($n) { $s = Get-Service $n -ErrorAction SilentlyContinue; if ($s -and $s.Status -eq 'Running') { Stop-Service $n -Force -ErrorAction SilentlyContinue; Log "Servicio $($n): detenido" } }
function Skip($x) { $x.Count -gt 5 -and $x[5] -and !(Test-Path -LiteralPath $x[0]) }
# Un junction/symlink, o algo que creó un usuario normal (ProgramData lo deja), no sirve para respaldos que se aplican
# como administrador: su dueño siempre puede volver a darse permisos. Solo vale lo que es de SYSTEM o Administradores.
function BkGuard($p) {
  $it = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
  if (!$it) { return }
  if ($it.Attributes -band [IO.FileAttributes]::ReparsePoint) { throw "$p es un enlace/reparse point: no se usa para respaldos" }
  $o = (Get-Acl -LiteralPath $p).GetOwner([Security.Principal.SecurityIdentifier]).Value
  if ($o -notin 'S-1-5-18', 'S-1-5-32-544') { throw "$p no pertenece a SYSTEM ni a Administradores (lo creó otro usuario): no se usa por seguridad" }
}
function Save-Bk($id, $obj) {
  $f = "$BK\$id.json"
  BkGuard (Split-Path $BK -Parent); BkGuard $BK; BkGuard $f
  if (Test-Path -LiteralPath $f) { Log "Ya hay un respaldo de '$id': se conservan los valores originales"; return }
  if (-not $script:BKREADY) {
    New-Item -ItemType Directory $BK -Force | Out-Null
    BkGuard (Split-Path $BK -Parent); BkGuard $BK
    # Solo SYSTEM y Administradores. Si icacls falla, no se escribe el respaldo (igual que en Rust).
    & ([Environment]::SystemDirectory + '\icacls.exe') $BK /inheritance:r /grant:r '*S-1-5-18:(OI)(CI)F' '*S-1-5-32-544:(OI)(CI)F' /T /Q | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "no se pudieron endurecer los permisos de $BK (icacls devolvió $LASTEXITCODE)" }
    $script:BKREADY = $true
  }
  ConvertTo-Json -InputObject $obj -Depth 4 | Set-Content -LiteralPath $f -Encoding UTF8
  Log "Respaldo guardado: $f ($(@($obj).Count) valores)"
}
function Read-Bk($id) { $f = "$BK\$id.json"; BkGuard $BK; BkGuard $f; if (Test-Path -LiteralPath $f) { $o = Get-Content -LiteralPath $f -Raw | ConvertFrom-Json; Log "Restaurando desde el respaldo $f"; return ,$o } }
function Remove-Bk($id) { $f = "$BK\$id.json"; if (Test-Path $f) { Remove-Item $f -Force -ErrorAction SilentlyContinue } }

function RegApply($id, $L) {
  if ($L.Count -eq 0) { return }
  Save-Bk $id @(foreach ($x in $L) { if (!(Skip $x)) { $c = Get-RegVal $x[0] $x[1]; [pscustomobject]@{ p = $x[0]; n = $x[1]; e = $c.e; v = $c.v; t = $c.t } } })
  foreach ($x in $L) { if (!(Skip $x)) { Set-RegVal $x[0] $x[1] $x[2] $x[3] } }
}
function SvcName($p) { if ($p -match '\\CurrentControlSet\\Services\\([^\\]+)$') { $Matches[1] } }
function StartSvc($n) { $s = Get-Service -Name $n -ErrorAction SilentlyContinue; if ($s -and $s.Status -ne 'Running') { try { Start-Service $n -ErrorAction Stop; Log "Servicio $($n): iniciado" } catch { Log "Servicio $($n): no se pudo iniciar ($($_.Exception.Message))" 'warn' } } }
function RegRevert($id, $L) {
  $b = Read-Bk $id; $done = @{}; $allowed = @{}
  # Solo se restauran claves del catálogo de este ajuste: un respaldo manipulado no puede escribir otra cosa.
  foreach ($x in $L) { if (!(Skip $x)) { $allowed["$($x[0])|$($x[1])".ToLower()] = 1 } }
  if ($null -eq $b -and $L.Count) { Log 'Sin respaldo: se vuelve a los valores por defecto de Windows' 'warn' }
  if ($null -ne $b) { foreach ($i in $b) {
    $key = "$($i.p)|$($i.n)".ToLower()
    if (!$allowed.ContainsKey($key)) { Log "El respaldo trae una entrada ajena al ajuste ($($i.p)\$($i.n)): se saltea" 'warn'; continue }
    if ($i.e -isnot [bool]) { Log "El respaldo trae una entrada sin estado: se saltea" 'warn'; continue }
    if ($i.e -and $i.t -notin 'DWord','QWord','String','ExpandString','MultiString','Binary') { Log "El respaldo trae un tipo de valor no soportado ($($i.t)): se saltea" 'warn'; continue }
    $done[$key] = 1
    if ($i.e) { Set-RegVal $i.p $i.n $i.v $i.t } else { Del-RegVal $i.p $i.n }
  } }
  # Lo que no está en el respaldo (respaldo viejo o incompleto) vuelve al valor por defecto de Windows.
  foreach ($x in $L) { if (!(Skip $x) -and !$done["$($x[0])|$($x[1])".ToLower()]) { if ($null -eq $x[4]) { Del-RegVal $x[0] $x[1] } else { Set-RegVal $x[0] $x[1] $x[4] $x[3] } } }
  # Un servicio que vuelve a Automático tiene que quedar corriendo; cambiar el Start solo no alcanza.
  if ($null -ne $b) { foreach ($i in $b) { if ($allowed.ContainsKey("$($i.p)|$($i.n)".ToLower()) -and $i.n -eq 'Start' -and $i.e -and [int]$i.v -eq 2) { $s = SvcName $i.p; if ($s) { StartSvc $s } } } }
  Remove-Bk $id
}
function RegCheck($L) {
  $n = 0
  foreach ($x in $L) { if (!(Skip $x)) { $n++; $c = Get-RegVal $x[0] $x[1]; if (!$c.e -or "$($c.v)" -ne "$($x[2])") { return $false } } }
  # Sin nada que revisar (servicios que no existen, lista vacía): $null = "no aplica en esta PC", no "apagado".
  if ($n) { $true }
}

# Planes de energía: lee el valor AC actual de un ajuste (independiente del idioma).
function PwrGet($sub, $set) {
  $m = [regex]::Matches((powercfg /qh SCHEME_CURRENT $sub $set | Out-String), '0x([0-9a-fA-F]{8})')
  if ($m.Count -ge 2) { [Convert]::ToInt32($m[$m.Count - 2].Groups[1].Value, 16) }
}
$PWN = @{ '48e6b7a6' = 'Suspensión selectiva USB'; 'ee12f906' = 'Ahorro de energía PCIe (ASPM)'; '0cc5b647' = 'Núcleos sin estacionar (%)';
  'be337238' = 'Modo boost del procesador'; '893dee8e' = 'Estado mínimo del procesador (%)'; '6738e2c4' = 'Apagar disco tras (s)' }
function PwrSet($sub, $set, $v) {
  $o = PwrGet $sub $set
  powercfg /setacvalueindex SCHEME_CURRENT $sub $set $v; if ($LASTEXITCODE) { throw "powercfg no pudo cambiar $set (código $LASTEXITCODE)" }
  powercfg /setdcvalueindex SCHEME_CURRENT $sub $set $v; if ($LASTEXITCODE) { throw "powercfg no pudo cambiar $set en batería (código $LASTEXITCODE)" }
  $nm = $PWN[$set.Substring(0, 8)]; if (!$nm) { $nm = $set }
  Log "Plan de energía · $($nm): $o → $v"
}
function PwrApply($id, $P) {
  Save-Bk $id @(foreach ($x in $P) { [pscustomobject]@{ s = $x[0]; k = $x[1]; v = (PwrGet $x[0] $x[1]) } })
  foreach ($x in $P) { PwrSet $x[0] $x[1] $x[2] }
  powercfg /setactive SCHEME_CURRENT
}
function PwrRevert($id, $P) {
  $b = Read-Bk $id; $allowed = @{}
  foreach ($x in $P) { $allowed["$($x[0])|$($x[1])".ToLower()] = 1 }
  if ($null -ne $b) { foreach ($i in $b) { if (!$allowed.ContainsKey("$($i.s)|$($i.k)".ToLower())) { Log 'El respaldo trae un ajuste de energía ajeno: se saltea' 'warn'; continue }; if ($null -ne $i.v) { PwrSet $i.s $i.k $i.v } } } else { foreach ($x in $P) { PwrSet $x[0] $x[1] $x[3] } }
  powercfg /setactive SCHEME_CURRENT
  Remove-Bk $id
}
function PwrSetAC($sub, $set, $v) {
  $o = PwrGet $sub $set; powercfg /setacvalueindex SCHEME_CURRENT $sub $set $v
  if ($LASTEXITCODE) { throw "powercfg no pudo cambiar $set (código $LASTEXITCODE)" }
  $nm = $PWN[$set.Substring(0, 8)]; if (!$nm) { $nm = $set }
  Log "Plan de energía (enchufada) · $($nm): $o → $v"
}
function PwrApplyAC($id, $P) {
  Save-Bk $id @(foreach ($x in $P) { [pscustomobject]@{ s = $x[0]; k = $x[1]; v = (PwrGet $x[0] $x[1]) } })
  foreach ($x in $P) { PwrSetAC $x[0] $x[1] $x[2] }
  powercfg /setactive SCHEME_CURRENT
}
function PwrRevertAC($id, $P) {
  $b = Read-Bk $id; $allowed = @{}
  foreach ($x in $P) { $allowed["$($x[0])|$($x[1])".ToLower()] = 1 }
  if ($null -ne $b) { foreach ($i in $b) { if (!$allowed.ContainsKey("$($i.s)|$($i.k)".ToLower())) { Log 'El respaldo trae un ajuste de energía ajeno: se saltea' 'warn'; continue }; if ($null -ne $i.v) { PwrSetAC $i.s $i.k $i.v } } } else { foreach ($x in $P) { PwrSetAC $x[0] $x[1] $x[3] } }
  powercfg /setactive SCHEME_CURRENT
  Remove-Bk $id
}
function PwrCheck($P) { foreach ($x in $P) { if ((PwrGet $x[0] $x[1]) -ne $x[2]) { return $false } }; $true }

# Tareas programadas y apps (se cachean para que el chequeo masivo sea rápido).
function AllTasks { if ($null -eq $script:TASKS) { $script:TASKS = @(Get-ScheduledTask -ErrorAction SilentlyContinue) }; $script:TASKS }
function FindTasks($pats) { AllTasks | Where-Object { $t = $_.TaskPath + $_.TaskName; @($pats | Where-Object { $t -like $_ }).Count -gt 0 } }
function TaskSet($pats, $on) {
  $list = @(FindTasks $pats); if (!$list.Count) { Log 'No hay tareas de este grupo en esta PC' }
  foreach ($t in $list) { $n = $t.TaskPath + $t.TaskName
    try { if ($on) { $t | Enable-ScheduledTask | Out-Null; Log "Tarea $($n): habilitada" } else { $t | Disable-ScheduledTask | Out-Null; Log "Tarea $($n): deshabilitada" } }
    catch { Log "Tarea $($n): protegida por Windows, se saltea" 'warn' } }
}
function TaskCheck($pats) { $f = @(FindTasks $pats); if ($f.Count) { @($f | Where-Object { $_.State -ne 'Disabled' }).Count -eq 0 } }
function Apps { if ($null -eq $script:APPS) { $script:APPS = @(Get-AppxPackage) }; $script:APPS }
function AppxFind($pats) { Apps | Where-Object { $n = $_.Name; @($pats | Where-Object { $n -like $_ }).Count -gt 0 } }
function AppxRemove($pats) {
  $list = @(AppxFind $pats); if (!$list.Count) { Log 'Ninguna app de este grupo está instalada: no hay nada que quitar' }
  foreach ($a in $list) { Log "Quitando app $($a.Name) $($a.Version)…"
    try { Remove-AppxPackage -Package $a.PackageFullName -ErrorAction Stop; Log "App $($a.Name): quitada" 'ok' }
    catch { Log "App $($a.Name): no se pudo quitar ($($_.Exception.Message))" 'warn' } }
}
function AppxRestore($pats) {
  foreach ($p in $pats) { $found = @(Get-AppxPackage -AllUsers -Name $p -ErrorAction SilentlyContinue | Where-Object InstallLocation)
    if (!$found.Count -and $p -notmatch '\*') { Log "App $($p): ya no está en el sistema, bajala de Microsoft Store" 'warn' }
    foreach ($a in $found) { Log "Reinstalando app $($a.Name)…"
      try { Add-AppxPackage -DisableDevelopmentMode -Register "$($a.InstallLocation)\AppXManifest.xml" -ErrorAction Stop; Log "App $($a.Name): reinstalada" 'ok' }
      catch { Log "App $($a.Name): no se pudo reinstalar ($($_.Exception.Message))" 'warn' } } }
}
function AppxCheck($pats) { @(AppxFind $pats).Count -eq 0 }

# Placa de red: propiedades avanzadas por palabra clave estándar (no dependen del idioma del driver).
function Nics { @(Get-NetAdapter -Physical -ErrorAction SilentlyContinue | Where-Object Status -eq 'Up') }
# Solo las propiedades que el driver tiene y que aceptan el valor buscado.
function NicProps($kv) { foreach ($a in Nics) { foreach ($k in $kv.Keys) { $p = Get-NetAdapterAdvancedProperty -Name $a.Name -RegistryKeyword $k -ErrorAction SilentlyContinue
  if ($p -and (!$p.ValidRegistryValues -or $p.ValidRegistryValues -contains $kv[$k])) { [pscustomobject]@{ a = $a.Name; k = $k; v = [string]$p.RegistryValue[0] } } } } }
# Tras reiniciar el adaptador hay que esperar a que vuelva a estar "Up"; si no, el chequeo no lo ve.
function WaitNics($n) {
  Log 'Esperando a que la conexión vuelva…'
  for ($i = 0; $i -lt 30 -and @(Nics).Count -lt $n; $i++) { Start-Sleep 1 }
  if (@(Nics).Count -ge $n) { Log "Conexión de red lista ($i s)" 'ok' } else { Log 'La red tarda en volver: revisá el cable o reiniciá' 'warn' }
}
function NicRestart($names) { foreach ($a in $names) { Log "Reiniciando adaptador $a (la conexión se corta unos segundos)…"; Restart-NetAdapter -Name $a -ErrorAction SilentlyContinue } }
function NicApply($id, $kv) {
  $cur = @(NicProps $kv); Save-Bk "$id-nic" $cur; $n = @(Nics).Count
  if (!$cur.Count) { Log 'Tu placa de red no tiene estas opciones avanzadas: no hay nada que cambiar ahí'; return }
  foreach ($c in $cur) { Set-NetAdapterAdvancedProperty -Name $c.a -RegistryKeyword $c.k -RegistryValue $kv[$c.k] -NoRestart; Log "Placa $($c.a) · $($c.k): $($c.v) → $($kv[$c.k])" }
  NicRestart @($cur.a | Select-Object -Unique); WaitNics $n
}
function NicRevert($id, $kv) {
  $b = Read-Bk "$id-nic"; $n = @(Nics).Count
  # Sin respaldo (aplicado con otra versión o a mano): lo que está como el tweak vuelve al valor por defecto del driver.
  if ($null -eq $b -or !@($b).Count) {
    Remove-Bk "$id-nic"; $cur = @(NicProps $kv | Where-Object { $_.v -eq [string]$kv[$_.k] }); if (!$cur.Count) { return }
    Log 'No hay respaldo de la placa de red: se vuelve a los valores por defecto del driver' 'warn'
    foreach ($c in $cur) { $d = [string](Get-NetAdapterAdvancedProperty -Name $c.a -RegistryKeyword $c.k).DefaultRegistryValue
      if ($d) { Set-NetAdapterAdvancedProperty -Name $c.a -RegistryKeyword $c.k -RegistryValue $d -NoRestart; Log "Placa $($c.a) · $($c.k): $($c.v) → $d (por defecto)" } }
    NicRestart @($cur.a | Select-Object -Unique); WaitNics $n; return
  }
  $fail = 0
  foreach ($c in $b) { if (!$kv.ContainsKey($c.k)) { Log "El respaldo trae una propiedad de red ajena ($($c.k)): se saltea" 'warn'; continue }
    try { Set-NetAdapterAdvancedProperty -Name $c.a -RegistryKeyword $c.k -RegistryValue $c.v -NoRestart -ErrorAction Stop; Log "Placa $($c.a) · $($c.k): → $($c.v)" }
    catch { $fail++; Log "Placa $($c.a) · $($c.k): no se pudo restaurar ($($_.Exception.Message))" 'warn' } }
  NicRestart @($b.a | Select-Object -Unique); WaitNics $n
  # Si algo no volvió, el respaldo queda para reintentar: borrarlo perdería los valores originales.
  if ($fail) { throw "$fail propiedades de red no se pudieron restaurar; el respaldo se conserva para reintentar" }
  Remove-Bk "$id-nic"
}
# Cada driver expone distintas propiedades: solo se exige que las que existen tengan el valor buscado. Sin ninguna, $null.
function NicCheck($kv) { $p = @(NicProps $kv); if ($p.Count) { @($p | Where-Object { $_.v -ne [string]$kv[$_.k] }).Count -eq 0 } }
`;

const P_SUB = '54533251-82be-4824-96c1-47b60b740d00'; // procesador
const GAMES_IFEO = 'HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Image File Execution Options/';
const game = (id, title, exe) => ({
  id, cat: 'games', name: `Prioridad alta para ${title}`, reg: [
    R(GAMES_IFEO + exe + '/PerfOptions', 'CpuPriorityClass', 3),
    R(GAMES_IFEO + exe + '/PerfOptions', 'IoPriority', 3)],
  desc: `Windows arranca ${exe} con prioridad de CPU y de disco alta. Menos tirones cuando hay programas abiertos de fondo.`,
});

// Campos: id, cat, name, desc, risk (low|med|high), reboot, rec(specs) => recomendado, needs(specs) => aplica a esta PC
// reg: entradas de registro | L: lista de registro calculada en PowerShell | apply/revert/check: PowerShell extra
const TWEAKS = [
  // ───────────── FPS y latencia ─────────────
  { id: 'ultimate', needs: s => !s.laptop && !s.x3dDual, cat: 'fps', name: 'Plan de energía Máximo rendimiento', rec: s => !s.laptop && !s.x3dDual,
    desc: 'Activa el plan "Ultimate Performance" oculto de Windows: CPU sin estados de ahorro y sin estacionar núcleos. Más FPS mínimos y menos micro-tirones. No se ofrece en Ryzen X3D de doble chiplet: el driver de AMD usa el estacionamiento de núcleos para mandar los juegos al chiplet con V-Cache.',
    apply: String.raw`$g='3f5c8a2e-6b1d-4e7a-9c0f-ff0000000001'; if(-not ((powercfg /list | Out-String) -match $g)){ powercfg -duplicatescheme e9a42b02-d5df-448d-aa00-03f14749eb61 $g | Out-Null; powercfg /changename $g 'Botox Optimizer Ultimate' | Out-Null }; powercfg /setactive $g`,
    revert: String.raw`powercfg /setactive 381b4222-f694-41f0-9685-ff5bb260df2e; if((powercfg /list | Out-String) -match '3f5c8a2e'){ powercfg /delete 3f5c8a2e-6b1d-4e7a-9c0f-ff0000000001 }`,
    check: String.raw`(powercfg /getactivescheme | Out-String) -match '3f5c8a2e'` },
  { id: 'gamemode', cat: 'fps', name: 'Activar Modo Juego de Windows', rec: () => true,
    desc: 'Windows le da prioridad al juego en primer plano y frena actualizaciones y tareas de fondo mientras jugás.',
    reg: [R('HKCU:/Software/Microsoft/GameBar', 'AutoGameModeEnabled', 1), R('HKCU:/Software/Microsoft/GameBar', 'AllowAutoGameMode', 1)] },
  { id: 'hags', cat: 'fps', name: 'Programación de GPU acelerada (HAGS)', reboot: true,
    needs: s => s.gpuVendor === 'NVIDIA' || s.gpuVendor === 'AMD', rec: s => s.gpuVendor === 'NVIDIA' || s.amdRdna2,
    desc: 'La GPU administra su propia cola de trabajo. Baja la latencia de entrada y es necesario para DLSS Frame Generation (NVIDIA). En Radeon se recomienda desde la RX 6000.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/GraphicsDrivers', 'HwSchMode', 2)] },
  { id: 'gamedvr', cat: 'fps', name: 'Desactivar Game DVR y grabación de fondo', rec: () => true,
    desc: 'Apaga la grabación en segundo plano de Xbox Game Bar, que consume GPU y disco mientras jugás.',
    reg: [R('HKCU:/System/GameConfigStore', 'GameDVR_Enabled', 0, 'DWord', 1),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/GameDVR', 'AppCaptureEnabled', 0),
      R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/GameDVR', 'AllowGameDVR', 0)] },
  { id: 'fso', cat: 'fps', name: 'Desactivar optimizaciones de pantalla completa', risk: 'med',
    desc: 'Fuerza pantalla completa exclusiva real en juegos DirectX 9/11. Puede bajar la latencia; en algunos juegos modernos no cambia nada.',
    reg: [R('HKCU:/System/GameConfigStore', 'GameDVR_FSEBehaviorMode', 2, 'DWord', 0),
      R('HKCU:/System/GameConfigStore', 'GameDVR_HonorUserFSEBehaviorMode', 1, 'DWord', 0),
      R('HKCU:/System/GameConfigStore', 'GameDVR_FSEBehavior', 2),
      R('HKCU:/System/GameConfigStore', 'GameDVR_DXGIHonorFSEWindowsCompatible', 1, 'DWord', 0)] },
  { id: 'windowedopt', cat: 'fps', name: 'Optimizaciones para juegos en ventana', rec: () => true,
    desc: 'Usa el modelo "flip" en juegos DX10/11 en ventana sin bordes: latencia similar a pantalla completa y permite VRR.',
    reg: [R('HKCU:/Software/Microsoft/DirectX/UserGpuPreferences', 'DirectXUserGlobalSettings', 'SwapEffectUpgradeEnable=1;', 'String')] },
  { id: 'sysresp', cat: 'fps', name: 'Reservar menos CPU para tareas de fondo', rec: () => true,
    desc: 'SystemResponsiveness de 20% a 10%: el programador multimedia (MMCSS) deja más CPU para el juego.',
    reg: [R('HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Multimedia/SystemProfile', 'SystemResponsiveness', 10, 'DWord', 20)] },
  { id: 'mmcss', cat: 'fps', name: 'Prioridad MMCSS alta para juegos', rec: () => true,
    desc: 'Sube la prioridad de CPU, GPU y E/S de la tarea "Games" del programador multimedia de Windows.',
    reg: [R('HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Multimedia/SystemProfile/Tasks/Games', 'GPU Priority', 8, 'DWord', 8),
      R('HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Multimedia/SystemProfile/Tasks/Games', 'Priority', 6, 'DWord', 2),
      R('HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Multimedia/SystemProfile/Tasks/Games', 'Scheduling Category', 'High', 'String', 'Medium'),
      R('HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Multimedia/SystemProfile/Tasks/Games', 'SFIO Priority', 'High', 'String', 'Normal')] },
  { id: 'win32prio', cat: 'fps', name: 'Priorizar el programa en primer plano', rec: () => true,
    desc: 'Win32PrioritySeparation = 0x26: quantums cortos y variables con más tiempo de CPU para la ventana activa (el juego).',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/PriorityControl', 'Win32PrioritySeparation', 38, 'DWord', 2)] },
  { id: 'powerthrottle', needs: s => !s.laptop, cat: 'fps', name: 'Desactivar Power Throttling', rec: s => !s.laptop,
    desc: 'Evita que Windows reduzca el rendimiento de procesos que considera "de fondo", como launchers, Discord u OBS.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/Power/PowerThrottling', 'PowerThrottlingOff', 1)] },
  { id: 'timerres', cat: 'fps', name: 'Resolución de timer global (Win11)', reboot: true, rec: () => true,
    desc: 'Restaura el comportamiento clásico del timer de alta resolución: los juegos mantienen frametimes estables aunque estén minimizados u ocultos.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/Session Manager/kernel', 'GlobalTimerResolutionRequests', 1)] },
  { id: 'dynamictick', cat: 'fps', name: 'Desactivar Dynamic Tick', risk: 'med', reboot: true,
    desc: 'El kernel deja de "saltear" ticks del reloj para ahorrar energía. Frametimes más consistentes en PC de escritorio.',
    // bcdedit no lanza excepciones: si falla, se revisa el código de salida y se muestra su mensaje.
    apply: `$o = bcdedit /set '{current}' disabledynamictick yes 2>&1; if ($LASTEXITCODE) { throw ('bcdedit: ' + (($o | Out-String).Trim() -replace '\\s+', ' ')) }`,
    revert: `$o = bcdedit /deletevalue '{current}' disabledynamictick 2>&1; if ($LASTEXITCODE) { throw ('bcdedit: ' + (($o | Out-String).Trim() -replace '\\s+', ' ')) }`,
    // "Yes" se traduce según el idioma de Windows; la línea solo existe si se configuró (revertir la borra).
    check: `(bcdedit /enum '{current}' | Out-String) -match 'disabledynamictick'` },
  { id: 'vbs', cat: 'fps', name: 'Desactivar VBS y Aislamiento del núcleo', risk: 'med', reboot: true,
    desc: 'La seguridad basada en virtualización puede costar 5-10% de FPS. Apaga también Credential Guard, que la mantiene prendida en Enterprise. Bajás una capa de protección de Windows a cambio.',
    // LsaCfgFlags: en Enterprise/Education, Credential Guard viene prendido por defecto y deja VBS corriendo aunque lo de arriba esté en 0.
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/DeviceGuard', 'EnableVirtualizationBasedSecurity', 0),
      R('HKLM:/SYSTEM/CurrentControlSet/Control/DeviceGuard/Scenarios/HypervisorEnforcedCodeIntegrity', 'Enabled', 0),
      R('HKLM:/SYSTEM/CurrentControlSet/Control/Lsa', 'LsaCfgFlags', 0)] },
  { id: 'mouseaccel', cat: 'fps', name: 'Desactivar aceleración del mouse', rec: () => true,
    desc: 'Apaga "Mejorar precisión del puntero": el movimiento del mouse es 1:1, clave para el aim en shooters.',
    reg: [R('HKCU:/Control Panel/Mouse', 'MouseSpeed', '0', 'String', '1'),
      R('HKCU:/Control Panel/Mouse', 'MouseThreshold1', '0', 'String', '6'),
      R('HKCU:/Control Panel/Mouse', 'MouseThreshold2', '0', 'String', '10')] },
  { id: 'keyboard', cat: 'fps', name: 'Respuesta de teclado más rápida',
    desc: 'Retardo de repetición al mínimo y velocidad de repetición al máximo.',
    reg: [R('HKCU:/Control Panel/Keyboard', 'KeyboardDelay', '0', 'String', '1'), R('HKCU:/Control Panel/Keyboard', 'KeyboardSpeed', '31', 'String', '31')] },
  { id: 'stickykeys', cat: 'fps', name: 'Desactivar atajos de Teclas especiales', rec: () => true,
    desc: 'Evita que apretar Shift 5 veces te saque del juego con el aviso de Teclas especiales, filtro y alternancia.',
    reg: [R('HKCU:/Control Panel/Accessibility/StickyKeys', 'Flags', '506', 'String', '510'),
      R('HKCU:/Control Panel/Accessibility/ToggleKeys', 'Flags', '58', 'String', '62'),
      R('HKCU:/Control Panel/Accessibility/Keyboard Response', 'Flags', '122', 'String', '126')] },
  { id: 'bgapps', cat: 'fps', name: 'Bloquear apps en segundo plano', rec: () => true,
    desc: 'Las apps de la Tienda dejan de ejecutarse de fondo: menos CPU, RAM y red ocupadas.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/BackgroundAccessApplications', 'GlobalUserDisabled', 1),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Search', 'BackgroundAppGlobalToggle', 0)] },
  { id: 'usbpower', needs: s => !s.laptop, cat: 'fps', name: 'Sin ahorro de energía en dispositivos USB', reboot: true, rec: s => !s.laptop,
    desc: 'Windows deja de suspender mouse, teclado y headset USB. Evita micro-cortes y el "primer clic" lento.',
    L: String.raw`@(Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Enum\USB' -Recurse -ErrorAction SilentlyContinue | Where-Object PSChildName -eq 'Device Parameters' | ForEach-Object { $p = 'HKLM:' + $_.Name.Substring(18); ,@($p,'EnhancedPowerManagementEnabled',0,'DWord',$null); ,@($p,'AllowIdleIrpInD3',0,'DWord',$null) })` },
  { id: 'fth', cat: 'fps', name: 'Desactivar Fault Tolerant Heap', rec: () => true,
    desc: 'Si un juego crasheó, Windows le aplica parches de memoria lentos para siempre. Esto lo impide.',
    reg: [R('HKLM:/SOFTWARE/Microsoft/FTH', 'Enabled', 0, 'DWord', 1)] },

  // ───────────── Energía ─────────────
  { id: 'hibernate', needs: s => !s.laptop, cat: 'power', name: 'Desactivar hibernación', rec: s => !s.laptop,
    desc: 'Borra hiberfil.sys (≈40% de tu RAM en el disco C:) y desactiva la hibernación.',
    why: s => `Con ${s.ramGB} GB de RAM libera ~${Math.round(s.ramGB * 0.4)} GB en C:`,
    apply: 'powercfg /h off', revert: 'powercfg /h on',
    check: String.raw`(Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\Power').HibernateEnabled -eq 0` },
  { id: 'faststartup', needs: s => !s.laptop, cat: 'power', name: 'Desactivar Inicio rápido', rec: s => !s.laptop,
    desc: 'El inicio rápido no reinicia el kernel del todo y arrastra problemas de drivers. Cada encendido arranca limpio.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/Session Manager/Power', 'HiberbootEnabled', 0, 'DWord', 1)] },
  { id: 'usbsuspend', needs: s => !s.laptop, cat: 'power', name: 'Desactivar suspensión selectiva USB', rec: s => !s.laptop,
    desc: 'El plan de energía deja de cortar la energía a los puertos USB inactivos.',
    pwr: [['2a737441-1930-4402-8d77-b2bebba308a3', '48e6b7a6-50f5-4782-a5d4-53bb8f07e226', 0, 1]] },
  { id: 'aspm', needs: s => !s.laptop, cat: 'power', name: 'Desactivar ahorro de energía PCIe (ASPM)', rec: s => !s.laptop,
    desc: 'La GPU y el NVMe no entran en estados de bajo consumo del bus PCIe: menos latencia al despertar.',
    pwr: [['501a4d13-42af-4429-9fd1-a8218c268e20', 'ee12f906-d277-404b-b6da-e5fa1a576df5', 0, 1]] },
  { id: 'coreparking', needs: s => !s.laptop && !s.x3dDual, cat: 'power', name: 'Desactivar estacionamiento de núcleos', rec: s => !s.laptop && !s.x3dDual,
    desc: 'Todos los núcleos quedan disponibles: sin la demora de "despertar" un núcleo dormido cuando el juego lo pide. No se ofrece en Ryzen X3D de doble chiplet: el driver de V-Cache de AMD necesita estacionar el chiplet sin caché mientras jugás.',
    pwr: [[P_SUB, '0cc5b647-c1df-4637-891a-dec35c318583', 100, 10]] },
  { id: 'cpuboost', needs: s => !s.laptop, cat: 'power', name: 'Boost del procesador agresivo',
    desc: 'El CPU sube a su frecuencia turbo lo más rápido posible. Más calor, mejor respuesta.',
    pwr: [[P_SUB, 'be337238-0d82-4146-a960-4f3749d470c7', 2, 1]] },
  { id: 'cpumin', needs: s => !s.laptop, cat: 'power', name: 'Estado mínimo del procesador al 100%', risk: 'med',
    desc: 'El CPU nunca baja de frecuencia. Mejores FPS mínimos; más consumo y temperatura en reposo.',
    pwr: [[P_SUB, '893dee8e-2bef-41e0-89c6-b55d0929964c', 100, 5]] },
  { id: 'diskidle', needs: s => !s.laptop, cat: 'power', name: 'No apagar los discos por inactividad', rec: s => !s.laptop,
    desc: 'Evita los tirones de 1-2 segundos cuando un disco dormido tiene que volver a girar o despertar.',
    pwr: [['0012ee47-9041-4b5d-9b77-535fba8b1442', '6738e2c4-e8a5-4a42-b16a-e040e769756e', 0, 1200]] },

  // ───────────── Red y ping ─────────────
  { id: 'netthrottle', cat: 'net', name: 'Desactivar limitación de red', rec: () => true,
    desc: 'NetworkThrottlingIndex = 0xFFFFFFFF: Windows deja de limitar paquetes de red mientras se reproduce audio o video.',
    reg: [R('HKLM:/SOFTWARE/Microsoft/Windows NT/CurrentVersion/Multimedia/SystemProfile', 'NetworkThrottlingIndex', -1, 'DWord', 10)] },
  { id: 'nagle', cat: 'net', name: 'Desactivar algoritmo de Nagle', rec: () => true,
    desc: 'Los paquetes TCP se envían al instante en vez de agruparse. Menos ping en juegos que usan TCP.',
    L: String.raw`@(Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces' | Where-Object { $i = Get-ItemProperty $_.PSPath; $i.DhcpIPAddress -or ($i.IPAddress -and $i.IPAddress[0]) } | ForEach-Object { $p = 'HKLM:' + $_.Name.Substring(18); ,@($p,'TcpAckFrequency',1,'DWord',$null); ,@($p,'TCPNoDelay',1,'DWord',$null); ,@($p,'TcpDelAckTicks',0,'DWord',$null) })` },
  { id: 'tcpparams', cat: 'net', name: 'Optimizar parámetros TCP/IP',
    desc: 'TTL 64, escalado de ventana TCP, más puertos efímeros y liberación rápida de conexiones cerradas.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Services/Tcpip/Parameters', 'DefaultTTL', 64),
      R('HKLM:/SYSTEM/CurrentControlSet/Services/Tcpip/Parameters', 'Tcp1323Opts', 1),
      R('HKLM:/SYSTEM/CurrentControlSet/Services/Tcpip/Parameters', 'MaxUserPort', 65534),
      R('HKLM:/SYSTEM/CurrentControlSet/Services/Tcpip/Parameters', 'TcpTimedWaitDelay', 30)] },
  { id: 'qos', cat: 'net', name: 'Liberar ancho de banda reservado (QoS)',
    desc: 'Pone en 0% el ancho de banda que la directiva QoS puede reservar para el sistema.',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/Psched', 'NonBestEffortLimit', 0)] },
  { id: 'deliveryopt', cat: 'net', name: 'Desactivar Optimización de distribución (P2P)', rec: () => true,
    desc: 'Windows deja de subir actualizaciones a otras PC por Internet. Libera tu subida y baja el bufferbloat mientras jugás.',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/DeliveryOptimization', 'DODownloadMode', 0)] },
  { id: 'nicpower', cat: 'net', name: 'Sin ahorro de energía en la placa de red', rec: () => true, reboot: true,
    desc: 'Desmarca "Permitir que el equipo apague este dispositivo" y apaga Energy Efficient Ethernet / Green Ethernet si el driver los tiene. La conexión puede cortarse unos segundos.',
    // Realtek: EEE/AdvancedEEE/EnableGreenEthernet/GigaLite/PowerSavingMode · Intel: EEELinkAdvertisement/ReduceSpeedOnPowerDown/ULPMode/SipsEnabled
    nic: { '*EEE': '0', 'EEE': '0', 'AdvancedEEE': '0', 'EnableGreenEthernet': '0', 'GigaLite': '0', 'PowerSavingMode': '0',
      'EEELinkAdvertisement': '0', 'ReduceSpeedOnPowerDown': '0', 'ULPMode': '0', 'SipsEnabled': '0' },
    // PnPCapabilities = 24 en la clave del driver = "Permitir que el equipo apague este dispositivo" desmarcado.
    L: String.raw`@(foreach ($a in Nics) { $g = "$($a.InterfaceGuid)"; Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e972-e325-11ce-bfc1-08002be10318}' -ErrorAction SilentlyContinue | Where-Object { (Get-ItemProperty $_.PSPath -ErrorAction SilentlyContinue).NetCfgInstanceId -eq $g } | ForEach-Object { ,@(('HKLM:' + $_.Name.Substring(18)),'PnPCapabilities',24,'DWord',$null) } })` },
  { id: 'niclatency', cat: 'net', name: 'Placa de red en modo baja latencia', risk: 'med',
    desc: 'Desactiva Interrupt Moderation, Flow Control y Large Send Offload: cada paquete se procesa al instante. Usa un poco más de CPU.',
    nic: { '*InterruptModeration': '0', '*FlowControl': '0', '*LsoV2IPv4': '0', '*LsoV2IPv6': '0' } },
  { id: 'dns', cat: 'net', name: 'DNS rápido (Cloudflare 1.1.1.1)',
    desc: 'Usa 1.1.1.1 y 1.0.0.1 en tus adaptadores activos. Resoluciones DNS más rápidas al entrar a servidores y launchers.',
    apply: String.raw`$b=@(foreach($a in Nics){ $g=(Get-NetAdapter -Name $a.Name).InterfaceGuid; [pscustomobject]@{ i=$a.ifIndex; ns=[string](Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Services\Tcpip\Parameters\Interfaces\$g").NameServer } }); Save-Bk 'dns' $b; foreach($a in Nics){ Set-DnsClientServerAddress -InterfaceIndex $a.ifIndex -ServerAddresses '1.1.1.1','1.0.0.1' }; Clear-DnsClientCache`,
    revert: String.raw`$b=Read-Bk 'dns'; if($null -eq $b){ foreach($a in Nics){ Set-DnsClientServerAddress -InterfaceIndex $a.ifIndex -ResetServerAddresses } } else { foreach($x in $b){ if($x.ns){ Set-DnsClientServerAddress -InterfaceIndex $x.i -ServerAddresses ($x.ns -split '[ ,]+') } else { Set-DnsClientServerAddress -InterfaceIndex $x.i -ResetServerAddresses } } }; Remove-Bk 'dns'; Clear-DnsClientCache`,
    // @() obligatorio: con un solo adaptador PowerShell devuelve un CimInstance y su .Count queda vacío.
    pre: '$n=@(Nics)', check: String.raw`$n.Count -gt 0 -and @($n | Where-Object { @((Get-DnsClientServerAddress -InterfaceIndex $_.ifIndex -AddressFamily IPv4 -ErrorAction SilentlyContinue).ServerAddresses)[0] -ne '1.1.1.1' }).Count -eq 0` },

  // ───────────── GPU ─────────────
  { id: 'gpumsi', cat: 'gpu', name: 'Modo MSI para la GPU', reboot: true, needs: s => !!s.gpuVendor, rec: s => !!s.gpuVendor,
    desc: 'Message Signaled Interrupts: la GPU avisa al CPU sin compartir líneas de interrupción. Menos latencia de DPC.',
    L: String.raw`@(Get-PnpDevice -Class Display -PresentOnly -ErrorAction SilentlyContinue | Where-Object InstanceId -like 'PCI\*' | ForEach-Object { ,@("HKLM:\SYSTEM\CurrentControlSet\Enum\$($_.InstanceId)\Device Parameters\Interrupt Management\MessageSignaledInterruptProperties",'MSISupported',1,'DWord',$null) })` },
  { id: 'nvtelemetry', cat: 'gpu', name: 'Desactivar telemetría de NVIDIA', needs: s => s.gpuVendor === 'NVIDIA', rec: s => s.gpuVendor === 'NVIDIA',
    desc: 'Deshabilita las tareas programadas NvTm* y el contenedor de telemetría del driver NVIDIA.',
    reg: [S('NvTelemetryContainer', 4, 2)],
    apply: `TaskSet ${psArr(['\\NvTm*', '\\NVIDIA*\\NvTm*'])} $false`, revert: `TaskSet ${psArr(['\\NvTm*', '\\NVIDIA*\\NvTm*'])} $true`,
    check: `TaskCheck ${psArr(['\\NvTm*', '\\NVIDIA*\\NvTm*'])}` },
  { id: 'nodriverwu', cat: 'gpu', name: 'Windows Update no toca tus drivers', rec: s => s.gpuVendor === 'NVIDIA' || s.gpuVendor === 'AMD',
    desc: 'Evita que Windows Update reemplace tu driver de NVIDIA/AMD por una versión vieja o genérica.',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/WindowsUpdate', 'ExcludeWUDriversInQualityUpdate', 1)] },
  { id: 'mpo', cat: 'gpu', name: 'Desactivar Multiplane Overlay (MPO)', risk: 'med', reboot: true,
    desc: 'MPO causa parpadeos, pantallazos negros o tirones en algunas PC con NVIDIA o AMD, sobre todo con dos monitores. Activalo solo si tenés esos síntomas.',
    reg: [R('HKLM:/SOFTWARE/Microsoft/Windows/Dwm', 'OverlayTestMode', 5)] },
  // Sin verificar en una PC con Radeon. Los servicios son los que lista el wiki de RadeonSoftwareSlimmer (AUEPLauncher = Programa de
  // Experiencia de Usuario de AMD; AMD Crash Defender Service = reportes de fallos). A propósito NO se toca "AMD External Events Utility":
  // lo usa FreeSync y otras funciones de Radeon. Las tareas salen de instalaciones típicas de Adrenalin; las que no existen se saltean.
  { id: 'amdtelemetry', cat: 'gpu', name: 'Desactivar telemetría y tareas de fondo de AMD', needs: s => s.gpuVendor === 'AMD', rec: s => s.gpuVendor === 'AMD',
    desc: 'Apaga el Programa de Experiencia de Usuario y el servicio de reportes de fallos de AMD, y las tareas de Adrenalin que buscan actualizaciones. El driver, FreeSync y el panel siguen funcionando.',
    reg: [S('AUEPLauncher', 4, 2), S('AMD Crash Defender Service', 4, 2)],
    pre: '$T=' + psArr(['\\AMDInstallLauncher', '\\AMDLinkUpdate', '\\AMDAutoUpdate*', '\\ModifyLinkUpdate', '\\AMDScoSupportTypeUpdate']),
    apply: 'TaskSet $T $false', revert: 'TaskSet $T $true', check: 'TaskCheck $T' },

  // ───────────── Disco y memoria ─────────────
  { id: 'lastaccess', cat: 'mem', name: 'No registrar último acceso NTFS', rec: () => true,
    desc: 'NTFS deja de escribir la fecha de último acceso en cada archivo que se lee. Menos escrituras al cargar juegos.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/FileSystem', 'NtfsDisableLastAccessUpdate', -2147483647, 'DWord', -2147483646)] },
  { id: '8dot3', cat: 'mem', name: 'Desactivar nombres cortos 8.3',
    desc: 'NTFS deja de crear nombres estilo DOS (PROGRA~1) para cada archivo nuevo. Acelera carpetas con muchos archivos.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/FileSystem', 'NtfsDisable8dot3NameCreation', 1, 'DWord', 2)] },
  { id: 'ntfsmem', cat: 'mem', name: 'Más memoria para caché de NTFS', rec: s => s.ramGB >= 16, needs: s => s.ramGB >= 8, reboot: true,
    desc: 'Aumenta la memoria paginada que usa NTFS para caché de archivos. Útil con mucha RAM y muchas lecturas.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/FileSystem', 'NtfsMemoryUsage', 2)] },
  { id: 'pagingexec', cat: 'mem', name: 'Kernel siempre en RAM', rec: s => s.ramGB >= 16, needs: s => s.ramGB >= 8, reboot: true,
    desc: 'DisablePagingExecutive: los drivers y el kernel nunca se mandan al archivo de paginación.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control/Session Manager/Memory Management', 'DisablePagingExecutive', 1, 'DWord', 0)] },
  { id: 'memcompress', cat: 'mem', name: 'Desactivar compresión de memoria', rec: s => s.ramGB >= 32, needs: s => s.ramGB >= 16, reboot: true,
    desc: 'Con RAM de sobra no hace falta comprimir páginas: se ahorra CPU en cada acceso.',
    why: s => `Tenés ${s.ramGB} GB de RAM`,
    apply: 'Disable-MMAgent -MemoryCompression', revert: 'Enable-MMAgent -MemoryCompression', check: '-not (Get-MMAgent).MemoryCompression' },
  { id: 'svchost', cat: 'mem', name: 'Agrupar procesos svchost según tu RAM', reboot: true, rec: s => s.ramGB >= 16,
    desc: 'Ajusta SvcHostSplitThresholdInKB a tu RAM total: menos procesos svchost.exe y menos sobrecarga.',
    L: String.raw`@(,@('HKLM:\SYSTEM\CurrentControlSet\Control','SvcHostSplitThresholdInKB',[int]((Get-CimInstance Win32_ComputerSystem).TotalPhysicalMemory/1KB),'DWord',3670016))` },
  { id: 'sysmain', cat: 'mem', name: 'Desactivar SysMain (Superfetch)', rec: s => s.sysDiskSSD,
    desc: 'En SSD/NVMe la precarga de SysMain casi no ayuda y genera lecturas y uso de CPU de fondo.',
    reg: [S('SysMain', 4, 2)], apply: 'Stop-Service SysMain -Force -ErrorAction SilentlyContinue' },
  { id: 'wsearch', cat: 'mem', name: 'Desactivar indexación de búsqueda', risk: 'med',
    desc: 'Apaga Windows Search. Menos uso de disco de fondo; la búsqueda del menú Inicio va a ser más lenta.',
    reg: [S('WSearch', 4, 2)], apply: 'Stop-Service WSearch -Force -ErrorAction SilentlyContinue', revert: 'Start-Service WSearch -ErrorAction SilentlyContinue' },
  // En algunas builds de Win11 el servicio rechaza hasta la consulta (0x80070005) aun como admin: ahí no aplica.
  { id: 'reserved', cat: 'mem', name: 'Liberar almacenamiento reservado', needs: s => s.reservedOK, rec: s => s.reservedOK && s.cFreeGB < 40,
    desc: 'Windows reserva ~7 GB en C: para actualizaciones. Lo libera. Algunas versiones de Windows 11 no permiten cambiarlo.',
    why: s => `C: tiene solo ${s.cFreeGB} GB libres`,
    apply: 'Set-WindowsReservedStorageState -State Disabled', revert: 'Set-WindowsReservedStorageState -State Enabled',
    check: "(Get-WindowsReservedStorageState).ReservedStorageState -eq 'Disabled'" },

  // ───────────── Servicios ─────────────
  { id: 'svcunneeded', cat: 'svc', name: 'Desactivar servicios innecesarios', rec: () => true,
    desc: 'Fax, modo demo, mapas descargados, geolocalización, Insider, red de Media Player y seguimiento de vínculos.',
    reg: [S('Fax', 4, 3), S('RetailDemo', 4, 3), S('MapsBroker', 4, 2), S('lfsvc', 4, 3), S('wisvc', 4, 3), S('WMPNetworkSvc', 4, 3), S('TrkWks', 4, 2)] },
  { id: 'svcxbox', cat: 'svc', name: 'Desactivar servicios de Xbox Live', risk: 'med',
    desc: 'Apaga autenticación y guardado en la nube de Xbox. No lo actives si jugás con Game Pass o la app Xbox.',
    reg: [S('XblAuthManager', 4, 3), S('XblGameSave', 4, 3), S('XboxNetApiSvc', 4, 3)] },
  { id: 'svcprint', cat: 'svc', name: 'Desactivar cola de impresión', risk: 'med',
    desc: 'Si no tenés impresora, el servicio Spooler queda apagado (también cierra un vector de ataque conocido).',
    reg: [S('Spooler', 4, 2)], apply: 'Stop-Service Spooler -Force -ErrorAction SilentlyContinue', revert: 'Start-Service Spooler -ErrorAction SilentlyContinue' },
  { id: 'svcdiag', cat: 'svc', name: 'Desactivar Asistente de compatibilidad',
    desc: 'El servicio PcaSvc vigila cada programa que abrís para sugerir "modos de compatibilidad".',
    reg: [S('PcaSvc', 4, 3)] },

  // ───────────── Juegos ─────────────
  game('gvalorant', 'Valorant', 'VALORANT-Win64-Shipping.exe'),
  game('gcs2', 'Counter-Strike 2', 'cs2.exe'),
  game('gfortnite', 'Fortnite', 'FortniteClient-Win64-Shipping.exe'),
  game('gaion2', 'AION 2', 'AION2.exe'),

  // ───────────── Notebook (solo se muestran en notebooks) ─────────────
  { id: 'lap-ac', cat: 'laptop', name: 'Rendimiento máximo solo con cargador', needs: s => s.laptop, rec: s => s.laptop, ac: true,
    desc: 'Boost agresivo del CPU, núcleos sin estacionar y sin ahorro de PCIe/USB, pero solo enchufada. Con batería queda el plan de siempre, así no se gasta de más.',
    pwr: [[P_SUB, 'be337238-0d82-4146-a960-4f3749d470c7', 2, 1],
      [P_SUB, '0cc5b647-c1df-4637-891a-dec35c318583', 100, 10],
      ['501a4d13-42af-4429-9fd1-a8218c268e20', 'ee12f906-d277-404b-b6da-e5fa1a576df5', 0, 1],
      ['2a737441-1930-4402-8d77-b2bebba308a3', '48e6b7a6-50f5-4782-a5d4-53bb8f07e226', 0, 1]] },
  // Va en la categoría Juegos (sirve también en PC de escritorio con dos GPU); el id se conserva por los respaldos ya guardados.
  // shortcut: solo busca los juegos en las rutas de instalación habituales, ampliar si hace falta
  { id: 'lap-dgpu', cat: 'games', name: 'Usar la GPU dedicada en los juegos', rec: s => s.laptop,
    desc: 'En notebooks y PCs con dos GPU, Windows a veces corre el juego en la integrada. Esto fuerza la GPU de alto rendimiento para Valorant, CS2, Fortnite y AION 2 si están instalados.',
    L: String.raw`@(foreach($d in @([IO.DriveInfo]::GetDrives() | Where-Object { $_.DriveType -eq 'Fixed' -and $_.IsReady } | ForEach-Object { $_.RootDirectory.FullName })){ foreach($r in @('Riot Games\VALORANT\live\ShooterGame\Binaries\Win64\VALORANT-Win64-Shipping.exe','Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe','Program Files\Epic Games\Fortnite\FortniteGame\Binaries\Win64\FortniteClient-Win64-Shipping.exe','SteamLibrary\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe','Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe','Program Files (x86)\Steam\steamapps\common\Counter-Strike Global Offensive\game\bin\win64\cs2.exe','SteamLibrary\steamapps\common\AION2\Aion2\Binaries\Win64\AION2.exe','Steam\steamapps\common\AION2\Aion2\Binaries\Win64\AION2.exe','Program Files (x86)\Steam\steamapps\common\AION2\Aion2\Binaries\Win64\AION2.exe')){ $f=$d+$r; if(Test-Path -LiteralPath $f){ ,@('HKCU:\Software\Microsoft\DirectX\UserGpuPreferences',$f,'GpuPreference=2;','String',$null) } } })`,
    apply: `if(-not @($L).Count){ Log 'No encontré Valorant, CS2, Fortnite ni AION 2 en las rutas habituales' 'warn' }`,
    check: '@($L).Count -gt 0' },

  // ───────────── Privacidad y telemetría ─────────────
  { id: 'diagtrack', cat: 'privacy', name: 'Desactivar telemetría (DiagTrack)', rec: () => true,
    desc: 'Apaga los servicios que recolectan y envían datos de uso y diagnóstico a Microsoft.',
    reg: [S('DiagTrack', 4, 2), S('dmwappushservice', 4, 3)], apply: 'Stop-Service DiagTrack -Force -ErrorAction SilentlyContinue' },
  { id: 'telemetrypol', cat: 'privacy', name: 'Telemetría al mínimo por directiva', rec: () => true,
    desc: 'AllowTelemetry = 0 (nivel Seguridad, disponible en Enterprise/Education).',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/DataCollection', 'AllowTelemetry', 0)] },
  { id: 'telemetrytasks', cat: 'privacy', name: 'Desactivar tareas de telemetría', rec: () => true,
    desc: 'Compatibility Appraiser, CEIP, diagnóstico de disco y Feedback: tareas que corren de fondo y usan disco.',
    apply: `TaskSet $T $false`, revert: `TaskSet $T $true`, check: `TaskCheck $T`,
    // Por nombre: SdbinstMergeDbTask (misma carpeta) está protegida y no es telemetría.
    pre: '$T=' + psArr(['\\Microsoft\\Windows\\Application Experience\\Microsoft Compatibility Appraiser*', '\\Microsoft\\Windows\\Application Experience\\ProgramDataUpdater',
      '\\Microsoft\\Windows\\Application Experience\\MareBackup', '\\Microsoft\\Windows\\Application Experience\\PcaPatchDbTask',
      '\\Microsoft\\Windows\\Application Experience\\StartupAppTask', '\\Microsoft\\Windows\\Customer Experience Improvement Program\\*',
      '\\Microsoft\\Windows\\Autochk\\Proxy', '\\Microsoft\\Windows\\DiskDiagnostic\\Microsoft-Windows-DiskDiagnosticDataCollector',
      '\\Microsoft\\Windows\\Feedback\\Siuf\\*']) },
  { id: 'adid', cat: 'privacy', name: 'Desactivar ID de publicidad',
    desc: 'Las apps dejan de usar tu ID de publicidad para anuncios personalizados.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/AdvertisingInfo', 'Enabled', 0, 'DWord', 1)] },
  { id: 'activity', cat: 'privacy', name: 'Desactivar historial de actividad',
    desc: 'Windows deja de guardar y subir qué apps y archivos usás.',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/System', 'EnableActivityFeed', 0),
      R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/System', 'PublishUserActivities', 0),
      R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/System', 'UploadUserActivities', 0)] },
  { id: 'wer', cat: 'privacy', name: 'Desactivar Informe de errores',
    desc: 'Cuando un juego crashea, Windows no arma ni sube un reporte (ahorra CPU y disco en ese momento).',
    reg: [R('HKLM:/SOFTWARE/Microsoft/Windows/Windows Error Reporting', 'Disabled', 1), S('WerSvc', 4, 3)] },
  { id: 'tailored', cat: 'privacy', name: 'Sin experiencias personalizadas ni encuestas',
    desc: 'Desactiva sugerencias basadas en tus datos de diagnóstico y las encuestas de Feedback.',
    reg: [R('HKCU:/Software/Microsoft/Siuf/Rules', 'NumberOfSIUFInPeriod', 0),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Privacy', 'TailoredExperiencesWithDiagnosticDataEnabled', 0)] },
  { id: 'copilot', cat: 'privacy', name: 'Desactivar Copilot y Recall', rec: () => true,
    desc: 'Apaga Copilot de Windows y las capturas de pantalla de Recall.',
    reg: [R('HKCU:/Software/Policies/Microsoft/Windows/WindowsCopilot', 'TurnOffWindowsCopilot', 1),
      R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/WindowsAI', 'DisableAIDataAnalysis', 1),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Explorer/Advanced', 'ShowCopilotButton', 0)] },
  { id: 'bing', cat: 'privacy', name: 'Quitar Bing del menú Inicio', rec: () => true,
    desc: 'La búsqueda del menú Inicio queda solo local: más rápida y sin enviar lo que escribís.',
    reg: [R('HKCU:/Software/Policies/Microsoft/Windows/Explorer', 'DisableSearchBoxSuggestions', 1),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Search', 'BingSearchEnabled', 0)] },
  { id: 'cortana', cat: 'privacy', name: 'Desactivar Cortana',
    desc: 'Bloquea Cortana por directiva para que no corra ni use CPU/disco.',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/Windows Search', 'AllowCortana', 0)] },
  { id: 'firefox', cat: 'privacy', name: 'Desactivar telemetría de Firefox',
    desc: 'Firefox deja de enviar datos de uso y rendimiento a Mozilla, y se apaga su agente de navegador predeterminado.',
    reg: [R('HKLM:/SOFTWARE/Policies/Mozilla/Firefox', 'DisableTelemetry', 1), R('HKLM:/SOFTWARE/Policies/Mozilla/Firefox', 'DisableDefaultBrowserAgent', 1)] },

  // ───────────── Calidad de vida ─────────────
  { id: 'visualfx', cat: 'qol', name: 'Efectos visuales en modo rendimiento', rec: () => true,
    desc: 'Desactiva animaciones, sombras y fundidos de Windows; mantiene las fuentes suavizadas. Requiere cerrar sesión.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Explorer/VisualEffects', 'VisualFXSetting', 3),
      R('HKCU:/Control Panel/Desktop', 'UserPreferencesMask', [0x90, 0x12, 0x03, 0x80, 0x10, 0x00, 0x00, 0x00], 'Binary'),
      R('HKCU:/Control Panel/Desktop/WindowMetrics', 'MinAnimate', '0', 'String', '1'),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Explorer/Advanced', 'TaskbarAnimations', 0)], reboot: true },
  { id: 'transparency', cat: 'qol', name: 'Desactivar transparencias',
    desc: 'Quita el efecto acrílico de la barra de tareas y el menú Inicio: menos trabajo para el compositor (DWM).',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Themes/Personalize', 'EnableTransparency', 0, 'DWord', 1)] },
  { id: 'menudelay', cat: 'qol', name: 'Menús instantáneos',
    desc: 'Los submenús se abren sin el retardo de 400 ms.',
    reg: [R('HKCU:/Control Panel/Desktop', 'MenuShowDelay', '0', 'String', '400')] },
  { id: 'startupdelay', cat: 'qol', name: 'Quitar retardo de inicio de apps', rec: () => true,
    desc: 'Windows deja de esperar ~10 s antes de abrir los programas de inicio.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Explorer/Serialize', 'StartupDelayInMSec', 0)] },
  { id: 'shutdown', cat: 'qol', name: 'Apagado más rápido',
    desc: 'Reduce a 2 s lo que Windows espera a que cierren apps y servicios colgados al apagar.',
    reg: [R('HKLM:/SYSTEM/CurrentControlSet/Control', 'WaitToKillServiceTimeout', '2000', 'String', '5000'),
      R('HKCU:/Control Panel/Desktop', 'AutoEndTasks', '1', 'String'),
      R('HKCU:/Control Panel/Desktop', 'HungAppTimeout', '2000', 'String'),
      R('HKCU:/Control Panel/Desktop', 'WaitToKillAppTimeout', '2000', 'String')] },
  { id: 'endtask', cat: 'qol', name: '"Finalizar tarea" en la barra de tareas', rec: () => true,
    desc: 'Clic derecho en un juego colgado en la barra de tareas → Finalizar tarea. Sin abrir el Administrador.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Explorer/Advanced/TaskbarDeveloperSettings', 'TaskbarEndTask', 1)] },
  { id: 'fileext', cat: 'qol', name: 'Mostrar extensiones de archivo',
    desc: 'Muestra .exe, .zip, etc. Ayuda a detectar archivos falsos.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/Explorer/Advanced', 'HideFileExt', 0, 'DWord', 1)] },
  { id: 'ads', cat: 'qol', name: 'Quitar anuncios y sugerencias de Windows', rec: () => true,
    desc: 'Sin apps sugeridas en Inicio, sin tips en Configuración y sin instalaciones silenciosas de apps promocionadas.',
    reg: ['SubscribedContent-338393Enabled', 'SubscribedContent-353694Enabled', 'SubscribedContent-353696Enabled', 'SubscribedContent-338388Enabled',
      'SubscribedContent-310093Enabled', 'SystemPaneSuggestionsEnabled', 'SilentInstalledAppsEnabled', 'SoftLandingEnabled']
      .map(n => R('HKCU:/Software/Microsoft/Windows/CurrentVersion/ContentDeliveryManager', n, 0, 'DWord', 1))
      .concat([R('HKCU:/Software/Microsoft/Windows/CurrentVersion/UserProfileEngagement', 'ScoobeSystemSettingEnabled', 0)]) },
  { id: 'lockads', cat: 'qol', name: 'Quitar publicidad de la pantalla de bloqueo',
    desc: 'Sin "datos curiosos" ni anuncios sobre la imagen de bloqueo.',
    reg: [R('HKCU:/Software/Microsoft/Windows/CurrentVersion/ContentDeliveryManager', 'RotatingLockScreenOverlayEnabled', 0, 'DWord', 1),
      R('HKCU:/Software/Microsoft/Windows/CurrentVersion/ContentDeliveryManager', 'SubscribedContent-338387Enabled', 0, 'DWord', 1)] },
  { id: 'classicmenu', cat: 'qol', name: 'Menú contextual clásico (Win11)', reboot: true,
    desc: 'Clic derecho muestra el menú completo de siempre, sin "Mostrar más opciones".',
    apply: String.raw`New-Item -Path 'HKCU:\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32' -Value '' -Force | Out-Null`,
    revert: String.raw`Remove-Item -LiteralPath 'HKCU:\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}' -Recurse -Force -ErrorAction SilentlyContinue`,
    check: String.raw`Test-Path -LiteralPath 'HKCU:\Software\Classes\CLSID\{86ca1aa0-34aa-4e8b-a509-50c905bae2a2}\InprocServer32'` },
  // La directiva AllowNewsAndInterests está protegida por UCPD (Windows bloquea escribirla desde scripts), así que se quita el paquete.
  { id: 'widgets', cat: 'qol', name: 'Quitar Widgets', rec: () => true,
    desc: 'Desinstala el panel de Widgets/Noticias (Windows Web Experience Pack) y su proceso WebView de fondo. Revertir lo reinstala; si no, bajalo de Microsoft Store.',
    apply: `AppxRemove @('MicrosoftWindows.Client.WebExperience')`, revert: `AppxRestore @('MicrosoftWindows.Client.WebExperience')`,
    check: `AppxCheck @('MicrosoftWindows.Client.WebExperience')` },
  { id: 'verboseboot', cat: 'qol', name: 'Mostrar información de arranque',
    desc: 'Muestra mensajes detallados al iniciar y apagar. No acelera, pero ayuda a ver qué se cuelga.',
    reg: [R('HKLM:/SOFTWARE/Microsoft/Windows/CurrentVersion/Policies/System', 'VerboseStatus', 1)] },
  { id: 'noautoreboot', cat: 'qol', name: 'Sin reinicios automáticos por updates', rec: () => true,
    desc: 'Windows Update no reinicia la PC solo mientras tenés la sesión abierta (ni en medio de una partida).',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/WindowsUpdate/AU', 'NoAutoRebootWithLoggedOnUsers', 1)] },

  // ───────────── Debloat ─────────────
  ...[
    ['dbnews', 'Quitar Noticias y El Tiempo', ['Microsoft.BingNews', 'Microsoft.BingWeather'], 'Apps de MSN que se actualizan solas de fondo.'],
    ['dbgames', 'Quitar juegos preinstalados', ['Microsoft.MicrosoftSolitaireCollection', 'king.com.*', '*CandyCrush*', 'Microsoft.ZuneVideo'], 'Solitaire, Candy Crush y Películas y TV.'],
    ['dbphone', 'Quitar Vínculo móvil', ['Microsoft.YourPhone', 'Microsoft.Windows.PhoneLink'], 'Phone Link corre de fondo esperando tu celular.', true],
    ['dbcopilot', 'Quitar app de Copilot', ['Microsoft.Copilot', 'Microsoft.Windows.Ai.Copilot.Provider'], 'La app de Copilot y su proveedor.', true],
    ['dbclipchamp', 'Quitar Clipchamp', ['Clipchamp.Clipchamp'], 'Editor de video web preinstalado.'],
    ['dbteams', 'Quitar Teams personal y Outlook nuevo', ['MSTeams', 'MicrosoftTeams', 'Microsoft.OutlookForWindows'], 'Teams (consumo) y el nuevo Outlook, que arrancan con Windows.'],
    ['dboffice', 'Quitar Office Hub y To Do', ['Microsoft.MicrosoftOfficeHub', 'Microsoft.Todos'], 'Accesos promocionales de Microsoft 365.'],
    ['dbhelp', 'Quitar Ayuda, Sugerencias y Feedback', ['Microsoft.GetHelp', 'Microsoft.Getstarted', 'Microsoft.WindowsFeedbackHub'], 'Apps de ayuda y encuestas.', true],
    ['dbmaps', 'Quitar Mapas, Contactos y Cortana', ['Microsoft.WindowsMaps', 'Microsoft.People', 'Microsoft.549981C3F5F10', 'Microsoft.BingSearch'], 'Apps poco usadas que se actualizan de fondo.'],
    ['dbdev', 'Quitar Dev Home, Power Automate y Asistencia rápida', ['Microsoft.Windows.DevHome', 'Microsoft.PowerAutomateDesktop', 'MicrosoftCorporationII.QuickAssist'], 'Herramientas que la mayoría no usa.'],
    ['dbxbox', 'Quitar app Xbox (Game Bar se queda)', ['Microsoft.GamingApp', 'Microsoft.XboxApp', 'Microsoft.XboxSpeechToTextOverlay'], 'No lo actives si usás Game Pass. La Game Bar y el login de Xbox en juegos siguen funcionando.', false, 'med'],
  ].map(([id, name, pats, desc, rec, risk]) => ({
    id, cat: 'debloat', name, risk: risk || 'low', rec: rec ? () => true : undefined, debloat: true,
    desc: desc + ' Al revertir se reinstala si sigue en el sistema; si no, bajala de Microsoft Store.',
    apply: `AppxRemove ${psArr(pats)}`, revert: `AppxRestore ${psArr(pats)}`, check: `AppxCheck ${psArr(pats)}`,
    ...(id === 'dbphone' ? { reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Windows/System', 'EnableMmx', 0)] } : {}),
  })),
  { id: 'onedrive', cat: 'debloat', name: 'Desinstalar OneDrive', risk: 'med', debloat: true,
    desc: 'Quita OneDrive y su sincronización de fondo. Tus archivos de la carpeta OneDrive no se borran. Revertir lo reinstala con winget.',
    // La copia del perfil está en una carpeta que el usuario puede escribir: elevada, solo si la firmó Microsoft.
    apply: String.raw`Get-Process OneDrive -ErrorAction SilentlyContinue | Stop-Process -Force; $u=@("$env:SystemRoot\System32\OneDriveSetup.exe","$env:SystemRoot\SysWOW64\OneDriveSetup.exe","$env:LOCALAPPDATA\Microsoft\OneDrive\OneDriveSetup.exe") | Where-Object { (Test-Path -LiteralPath $_) -and (MsSigned $_) } | Select-Object -First 1; if($u){ Start-Process $u '/uninstall' -Wait } else { & (WingetExe) uninstall --id Microsoft.OneDrive -e --silent | Out-Null }`,
    revert: '& (WingetExe) install --id Microsoft.OneDrive -e --silent --accept-source-agreements --accept-package-agreements | Out-Null',
    check: String.raw`-not (Test-Path "$env:LOCALAPPDATA\Microsoft\OneDrive\OneDrive.exe") -and -not (Test-Path "$env:ProgramFiles\Microsoft OneDrive\OneDrive.exe")` },
  { id: 'edgebg', cat: 'debloat', name: 'Edge sin arranque ni segundo plano', rec: () => true,
    desc: 'Edge deja de precargarse al iniciar Windows y de quedar abierto después de cerrarlo.',
    reg: [R('HKLM:/SOFTWARE/Policies/Microsoft/Edge', 'StartupBoostEnabled', 0), R('HKLM:/SOFTWARE/Policies/Microsoft/Edge', 'BackgroundModeEnabled', 0)] },
];

const CATS = {
  fps: 'FPS y latencia', power: 'Energía', net: 'Red y ping', gpu: 'GPU', mem: 'Disco y memoria',
  svc: 'Servicios', games: 'Juegos', privacy: 'Privacidad', qol: 'Calidad de vida', debloat: 'Debloat', laptop: 'Notebook',
};

// ── Generadores de PowerShell ──
const lExpr = t => t.L || psList(t.reg || []); // un tweak usa L (dinámico) o reg (fijo), no ambos
const pwrList = t => psList(t.pwr || []);
const nicHash = t => '@{' + Object.entries(t.nic).map(([k, v]) => `${psq(k)}=${psq(v)}`).join(';') + '}';
const stops = t => (t.reg || []).filter(e => e[5] && e[2] === 4).map(e => `StopSvc '${e[0].split('\\').pop()}'`).join('; ');
// Muestra en la consola el comando propio del tweak (los helpers ya registran sus pasos).
const echo = cmd => /^(AppxRemove|AppxRestore|TaskSet)\b/.test(cmd) ? cmd
  : `Log ${psq('› ' + (cmd.length > 150 ? cmd.slice(0, 150) + '…' : cmd).replace(/\s+/g, ' '))} 'cmd'\n${cmd}`;

function applyPS(t) {
  const p = [t.pre || '', `$L=${lExpr(t)}; RegApply '${t.id}' $L`];
  if (t.pwr) p.push(`PwrApply${t.ac ? 'AC' : ''} '${t.id}' ${pwrList(t)}`);
  if (t.nic) p.push(`NicApply '${t.id}' ${nicHash(t)}`);
  if (t.reg) p.push(stops(t));
  if (t.apply) p.push(echo(t.apply));
  return p.filter(Boolean).join('\n');
}
function revertPS(t) {
  const p = [t.pre || ''];
  // RegRevert solo si el tweak tiene registro propio: si no, borraría el respaldo de un revert personalizado (dns).
  if (t.reg || t.L) p.push(`$L=${lExpr(t)}; RegRevert '${t.id}' $L`);
  if (t.pwr) p.push(`PwrRevert${t.ac ? 'AC' : ''} '${t.id}' ${pwrList(t)}`);
  if (t.nic) p.push(`NicRevert '${t.id}' ${nicHash(t)}`);
  if (t.revert) p.push(echo(t.revert));
  return p.filter(Boolean).join('\n');
}
function checkPS(t) {
  const c = [];
  if (t.reg || t.L) c.push('(RegCheck $L)');
  if (t.pwr) c.push(`(PwrCheck ${pwrList(t)})`);
  if (t.nic) c.push(`(NicCheck ${nicHash(t)})`);
  // RegCheck, NicCheck y TaskCheck dan $null si en esta PC no hay nada que revisar. Un chequeo propio se fuerza a bool.
  if (t.check) c.push(/^TaskCheck /.test(t.check) ? `(${t.check})` : `[bool](${t.check})`);
  // Si ninguna parte encontró qué revisar, 'na' (no aplica en esta PC); si no, activo = ninguna parte en falso.
  return `${t.pre || ''}\n$L=${lExpr(t)}\n$c=@(${c.join(', ') || '$false'} | Where-Object { $null -ne $_ })\nif (!$c.Count) { 'na' } else { !@($c | Where-Object { !$_ }).Count }`;
}
function checkAllPS(list = TWEAKS) {
  return '$r=[ordered]@{}\n' + list.map(t =>
    `try { $r['${t.id}'] = & { ${checkPS(t)} } } catch { $r['${t.id}'] = $false }`).join('\n') +
    '\n$r | ConvertTo-Json -Compress';
}

// ── Boost-Up: limpiezas y mantenimiento de una sola vez ──
const CLEAN = String.raw`function Clean($paths, $filter = '*') {
  $b = 0
  foreach ($p in $paths) {
    if (!(Test-Path $p)) { Log "$($p): no existe, se saltea"; continue }
    $f = @(Get-ChildItem $p -Recurse -Force -File -Filter $filter -ErrorAction SilentlyContinue); $s = ($f | Measure-Object Length -Sum).Sum
    Log ("{0}: {1} archivos, {2:N1} MB" -f $p, $f.Count, ($s / 1MB))
    if (!$f.Count) { continue }
    $f | Remove-Item -Force -ErrorAction SilentlyContinue
    $r = @(Get-ChildItem $p -Recurse -Force -File -Filter $filter -ErrorAction SilentlyContinue); $rs = ($r | Measure-Object Length -Sum).Sum; $b += $s - $rs
    Log ("  borrados {0} · en uso o protegidos {1} ({2:N1} MB, se saltean)" -f ($f.Count - $r.Count), $r.Count, ($rs / 1MB)) $(if ($r.Count) { 'info' } else { 'ok' })
  }
  'Liberados {0:N0} MB' -f ($b / 1MB)
}
`;
const BOOSTS = [
  { id: 'temp', name: 'Limpiar archivos temporales', icon: '🧹', every: 'Cada semana',
    desc: 'Borra los temporales de tu usuario y de Windows. Lo que está en uso se saltea.',
    ps: CLEAN + 'Clean @($env:TEMP, "$env:SystemRoot\\Temp")' },
  { id: 'shaders', name: 'Limpiar caché de shaders', icon: '🎮', every: 'Después de actualizar el driver de video',
    desc: 'Borra la caché de shaders de DirectX, NVIDIA y AMD. La primera partida después compila de nuevo (puede tironear un rato) y luego queda limpia.',
    ps: CLEAN + 'Clean @("$env:LOCALAPPDATA\\D3DSCache", "$env:LOCALAPPDATA\\NVIDIA\\DXCache", "$env:LOCALAPPDATA\\NVIDIA\\GLCache", "$env:LOCALAPPDATA\\AMD\\DxCache", "$env:LOCALAPPDATA\\AMD\\DxcCache")' },
  { id: 'thumbs', name: 'Limpiar caché de miniaturas', icon: '🖼️', every: 'Cada mes',
    desc: 'Borra las miniaturas guardadas del Explorador. Se regeneran solas.',
    ps: CLEAN + 'Clean @("$env:LOCALAPPDATA\\Microsoft\\Windows\\Explorer") "thumbcache_*.db"' },
  { id: 'dumps', name: 'Borrar volcados de errores', icon: '💥', every: 'Cada mes',
    desc: 'Borra minidumps y reportes de errores viejos de juegos y Windows.',
    ps: CLEAN + 'Clean @("$env:LOCALAPPDATA\\CrashDumps", "$env:SystemRoot\\Minidump", "$env:ProgramData\\Microsoft\\Windows\\WER\\ReportArchive", "$env:ProgramData\\Microsoft\\Windows\\WER\\ReportQueue")' },
  { id: 'wucache', name: 'Limpiar caché de Windows Update', icon: '📦', every: 'Cada mes',
    desc: 'Borra instaladores de actualizaciones ya descargadas. Útil cuando C: está lleno.',
    // try/finally: si la limpieza falla, los servicios se vuelven a levantar igual.
    ps: CLEAN + String.raw`Log 'Deteniendo Windows Update y BITS para poder borrar…'; Stop-Service wuauserv,bits -Force -ErrorAction SilentlyContinue
try { $m = Clean @("$env:SystemRoot\SoftwareDistribution\Download") }
finally { Start-Service wuauserv,bits -ErrorAction SilentlyContinue; Log 'Windows Update y BITS reanudados' 'ok' }
$m` },
  { id: 'recent', name: 'Limpiar documentos recientes', icon: '📄', every: 'Cuando quieras',
    desc: 'Vacía la lista de archivos recientes del Explorador y de Acceso rápido.',
    ps: CLEAN + 'Clean @("$env:APPDATA\\Microsoft\\Windows\\Recent") "*.lnk"' },
  { id: 'dns', name: 'Vaciar caché DNS', icon: '🌐', every: 'Si un juego no conecta',
    desc: 'Borra las direcciones DNS guardadas. Resuelve errores de conexión a servidores que cambiaron de IP.',
    ps: "$n = @(Get-DnsClientCache -ErrorAction SilentlyContinue).Count; Clear-DnsClientCache; Log \"Entradas en caché borradas: $n\" 'ok'; 'Caché DNS vaciada'" },
  { id: 'trim', name: 'Optimizar unidades (TRIM / desfragmentar)', icon: '💽', every: 'Cada semana',
    desc: 'TRIM en SSD/NVMe y desfragmentación en HDD, según el tipo de cada disco. En HDD puede tardar varios minutos.',
    ps: String.raw`$v = @(Get-Volume | Where-Object { $_.DriveType -eq 'Fixed' -and $_.DriveLetter } | Sort-Object DriveLetter)
foreach ($x in $v) { $d = "$($x.DriveLetter):"; $t0 = Get-Date
  Log "Optimizando $d $($x.FileSystemLabel)… (TRIM si es SSD, desfragmentación si es HDD)"
  try { Optimize-Volume -DriveLetter $x.DriveLetter -ErrorAction Stop; Log ("{0} listo en {1:N0} s" -f $d, ((Get-Date) - $t0).TotalSeconds) 'ok' }
  catch { Log "$($d) no se pudo optimizar: $($_.Exception.Message)" 'warn' } }
'Unidades procesadas: ' + (($v | ForEach-Object { "$($_.DriveLetter):" }) -join ' ')` },
  { id: 'dism', name: 'Limpiar componentes de Windows', icon: '🧩', every: 'Cada 2-3 meses',
    desc: 'DISM StartComponentCleanup: borra versiones viejas de componentes en WinSxS. Puede tardar 5-15 minutos.',
    ps: String.raw`Log 'DISM StartComponentCleanup en curso: suele tardar 5-15 minutos. El contador de la consola muestra que sigue trabajando.'
$sd = [IO.Path]::GetPathRoot([Environment]::SystemDirectory).Substring(0, 1)  # unidad de Windows (no siempre C:)
$a = (Get-PSDrive $sd).Free; & ([Environment]::SystemDirectory + '\Dism.exe') /Online /Cleanup-Image /StartComponentCleanup /Quiet | Out-Null; $c = $LASTEXITCODE
Log "DISM terminó (código $c)" $(if ($c) { 'warn' } else { 'ok' })
'Liberados {0:N0} MB en {1}:' -f (((Get-PSDrive $sd).Free - $a) / 1MB), $sd` },
];

module.exports = { TWEAKS, CATS, BOOSTS, PRELUDE, applyPS, revertPS, checkPS, checkAllPS };
