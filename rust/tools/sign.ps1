# Firma el .exe de release con el certificado de Botox Optimizer y muestra el SHA256 para la descripción del release.
# Uso: tools\sign.ps1 <ruta a BotoxOptimizer.exe>
param([Parameter(Mandatory)] [string]$Exe)
$ErrorActionPreference = 'Stop'
$c = @(Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object Subject -eq 'CN=Botox Optimizer')
if ($c.Count -ne 1) { throw "Se esperaba 1 certificado 'CN=Botox Optimizer' en CurrentUser\My y hay $($c.Count). Ver tools\new-signing-cert.ps1." }
$s = Set-AuthenticodeSignature -LiteralPath $Exe -Certificate $c[0] -HashAlgorithm SHA256 -TimestampServer 'http://timestamp.digicert.com'
# Con un certificado autofirmado Windows dice "UnknownError" (raíz no confiable): es lo esperado. Lo que importa es el firmante.
if (-not $s.SignerCertificate -or $s.SignerCertificate.Thumbprint -ne $c[0].Thumbprint) { throw "No se firmó: $($s.StatusMessage)" }
"Firmado por $($c[0].Thumbprint) (estado de Windows: $($s.Status))"
"SHA256: $((Get-FileHash -LiteralPath $Exe -Algorithm SHA256).Hash.ToLower())"
