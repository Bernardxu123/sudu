param(
    [Parameter(Mandatory = $true)][string]$Source,
    [Parameter(Mandatory = $true)][string]$Dest
)

Copy-Item -LiteralPath $Source -Destination $Dest -Force
icacls $Dest /setintegritylevel Medium | Out-Null
if ($LASTEXITCODE -ne 0) {
    throw "icacls failed for $Dest"
}
Write-Output ("packaged " + $Dest)
