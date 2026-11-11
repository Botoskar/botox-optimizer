// Escáner de hardware y estado del sistema. Solo lectura. Devuelve JSON.
module.exports = String.raw`
$ErrorActionPreference = 'SilentlyContinue'
[Console]::OutputEncoding = [Text.Encoding]::UTF8
$cpu = Get-CimInstance Win32_Processor | Select-Object -First 1
$os = Get-CimInstance Win32_OperatingSystem
$cs = Get-CimInstance Win32_ComputerSystem
$bb = Get-CimInstance Win32_BaseBoard
$bios = Get-CimInstance Win32_BIOS
$nt = Get-ItemProperty 'HKLM:\SOFTWARE\Microsoft\Windows NT\CurrentVersion'

# VRAM real (AdapterRAM se queda en 4 GB): se lee del registro del driver.
$vr = @{}
Get-ChildItem 'HKLM:\SYSTEM\CurrentControlSet\Control\Class\{4d36e968-e325-11ce-bfc1-08002be10318}' | ForEach-Object {
  $p = Get-ItemProperty $_.PSPath
  if ($p.DriverDesc -and $p.'HardwareInformation.qwMemorySize') { $vr[$p.DriverDesc] = [int64]$p.'HardwareInformation.qwMemorySize' }
}
$nvDriver = $null
if (Get-Command nvidia-smi -ErrorAction SilentlyContinue) { $nvDriver = (nvidia-smi --query-gpu=driver_version --format=csv,noheader | Select-Object -First 1) }
$gpus = @(Get-CimInstance Win32_VideoController | ForEach-Object {
  $b = if ($vr[$_.Name]) { $vr[$_.Name] } else { [int64]$_.AdapterRAM }
  [ordered]@{ name = $_.Name; vramGB = [math]::Round($b / 1GB, 1); driver = $(if ($nvDriver -and $_.Name -match 'NVIDIA') { $nvDriver } else { $_.DriverVersion });
    refresh = $_.CurrentRefreshRate; res = "$($_.CurrentHorizontalResolution)x$($_.CurrentVerticalResolution)" }
})

$mods = @(Get-CimInstance Win32_PhysicalMemory | ForEach-Object {
  [ordered]@{ gb = [math]::Round($_.Capacity / 1GB); speed = $_.Speed; conf = $_.ConfiguredClockSpeed; maker = "$($_.Manufacturer)".Trim(); part = "$($_.PartNumber)".Trim(); slot = $_.DeviceLocator; type = $_.SMBIOSMemoryType }
})

$parts = @(Get-Partition | Where-Object DriveLetter)
$disks = @(Get-PhysicalDisk | ForEach-Object {
  $id = $_.DeviceId
  [ordered]@{ name = $_.FriendlyName; media = "$($_.MediaType)"; bus = "$($_.BusType)"; sizeGB = [math]::Round($_.Size / 1GB)
    letters = @($parts | Where-Object { "$($_.DiskNumber)" -eq "$id" } | ForEach-Object { "$($_.DriveLetter)" }) }
})
$vols = @(Get-Volume | Where-Object { $_.DriveLetter -and $_.DriveType -eq 'Fixed' } | Sort-Object DriveLetter | ForEach-Object {
  [ordered]@{ letter = "$($_.DriveLetter)"; label = $_.FileSystemLabel; sizeGB = [math]::Round($_.Size / 1GB); freeGB = [math]::Round($_.SizeRemaining / 1GB) }
})

$nics = @(Get-NetAdapter -Physical | Where-Object Status -eq 'Up' | ForEach-Object {
  [ordered]@{ name = $_.Name; desc = $_.InterfaceDescription; speed = $_.LinkSpeed; wifi = ($_.PhysicalMediaType -match '802\.11');
    dns = ((Get-DnsClientServerAddress -InterfaceIndex $_.ifIndex -AddressFamily IPv4).ServerAddresses -join ', ') }
})

$dg = Get-CimInstance -Namespace root\Microsoft\Windows\DeviceGuard -ClassName Win32_DeviceGuard
$plan = (powercfg /getactivescheme | Out-String) -replace '(?s).*\((.*)\).*', '$1'
$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

[ordered]@{
  pc = $env:COMPUTERNAME
  admin = $admin
  laptop = [bool](Get-CimInstance Win32_Battery)
  cpu = [ordered]@{ name = "$($cpu.Name)".Trim(); maker = $cpu.Manufacturer; cores = $cpu.NumberOfCores; threads = $cpu.NumberOfLogicalProcessors; mhz = $cpu.MaxClockSpeed; l3MB = [math]::Round($cpu.L3CacheSize / 1024) }
  gpus = $gpus
  ram = [ordered]@{ gb = [math]::Round($cs.TotalPhysicalMemory / 1GB); modules = $mods }
  disks = $disks
  vols = $vols
  sysDrive = $env:SystemDrive.TrimEnd(':')
  nics = $nics
  board = "$($bb.Manufacturer) $($bb.Product)".Trim()
  bios = "$($bios.SMBIOSBIOSVersion) ($(if ($bios.ReleaseDate) { $bios.ReleaseDate.ToString('yyyy-MM-dd') }))"
  os = [ordered]@{ name = $os.Caption; ver = $nt.DisplayVersion; build = "$($os.BuildNumber).$($nt.UBR)"; uptimeH = [math]::Round(((Get-Date) - $os.LastBootUpTime).TotalHours, 1) }
  vbs = [int]$dg.VirtualizationBasedSecurityStatus
  vbsRunning = @($dg.SecurityServicesRunning | ForEach-Object { [int]$_ })
  hypervisor = [bool]$cs.HypervisorPresent
  hags = (Get-ItemProperty 'HKLM:\SYSTEM\CurrentControlSet\Control\GraphicsDrivers').HwSchMode
  plan = $plan.Trim()
  reserved = $(try { [string](Get-WindowsReservedStorageState -ErrorAction Stop).ReservedStorageState } catch { '' })
  planGuid =[regex]::Match((powercfg /getactivescheme | Out-String), '[0-9a-f]{8}-[0-9a-f-]{27}').Value
} | ConvertTo-Json -Depth 5 -Compress
`;
