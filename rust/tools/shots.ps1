# Saca una captura de cada pantalla de la app (solo lectura). Uso: tools\shots.ps1 <carpeta de salida> [debug|release]
param([Parameter(Mandatory)] [string]$Out, [string]$Profile = 'debug')
if ($Profile -ne 'debug') { throw 'Las capturas usan BX_SHOT/BX_PAGE, que el .exe de release ignora a propósito: usá el build debug.' }
$exe = "M:\Github Things\botox-optimizer-rust\target\$Profile\botox-optimizer.exe"
New-Item -ItemType Directory $Out -Force | Out-Null
$shots = @(
  @{ n = 'home'; page = 'home' }, @{ n = 'scan'; page = 'scan' }, @{ n = 'tweaks'; page = 'tweaks' }, @{ n = 'fix'; page = 'fix' },
  @{ n = 'debloat'; page = 'debloat' }, @{ n = 'boost'; page = 'boost'; demo = '1' }, @{ n = 'backup'; page = 'backup' },
  @{ n = 'notebook'; page = 'tweaks'; tab = 'laptop'; laptop = '1' }
)
foreach ($s in $shots) {
  $env:BX_SHOT = "$Out\$($s.n).png"; $env:BX_PAGE = $s.page
  $env:BX_TAB = $s.tab; $env:BX_LAPTOP = $s.laptop; $env:BX_DEMO = $s.demo
  $p = Start-Process $exe -PassThru
  if (-not $p.WaitForExit(90000)) { $p.Kill(); "timeout: $($s.n)" }
}
Remove-Item Env:BX_SHOT, Env:BX_PAGE, Env:BX_TAB, Env:BX_LAPTOP, Env:BX_DEMO -ErrorAction SilentlyContinue
Get-ChildItem $Out\*.png | Select-Object Name, Length
