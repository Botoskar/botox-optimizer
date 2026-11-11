# Crea el certificado autofirmado de firma de código de Botox Optimizer en Cert:\CurrentUser\My (una sola vez).
# No lo agrega a las raíces de confianza: la app lo reconoce por su thumbprint, fijado en rust/src/update.rs.
$ErrorActionPreference = 'Stop'
$subject = 'CN=Botox Optimizer'
$existing = @(Get-ChildItem Cert:\CurrentUser\My -CodeSigningCert | Where-Object Subject -eq $subject)
if ($existing.Count) { throw "Ya existe ($($existing.Thumbprint -join ', ')). Usá ese: uno nuevo no lo aceptan las versiones ya publicadas." }
$c = New-SelfSignedCertificate -Type CodeSigningCert -Subject $subject -CertStoreLocation Cert:\CurrentUser\My `
  -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -KeyExportPolicy Exportable -NotAfter (Get-Date).AddYears(20)
"Thumbprint (va en PUBLISHER_THUMBPRINTS): $($c.Thumbprint)"
"Vence: $($c.NotAfter.ToString('yyyy-MM-dd'))"
'Hacé una copia de la clave fuera de esta PC (ver README > Sacar una versión nueva). Si se pierde, la app ya no se actualiza sola.'
