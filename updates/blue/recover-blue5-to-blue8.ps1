param(
    [string]$TargetDir
)

$ErrorActionPreference = 'Stop'
$Version = '0.1.9-blue.8'
$ExpectedSha256 = '6c6bc363a09d7d4669ed36bfb7368fc12c1c3be8c56f95524b0cca84b1239981'
$ExpectedSize = 54458880
$DownloadUrl = 'https://github.com/glitchaka/adm-toolbox/releases/download/sst-v0.1.9-blue.8/sst.exe'

function Resolve-SstTarget {
    param([string]$ExplicitDir)

    if (-not [string]::IsNullOrWhiteSpace($ExplicitDir)) {
        $candidate = Join-Path ([System.IO.Path]::GetFullPath($ExplicitDir)) 'sst.exe'
        if (Test-Path -LiteralPath $candidate -PathType Leaf) { return $candidate }
        throw "No existe sst.exe en $ExplicitDir"
    }

    $cwdCandidate = Join-Path (Get-Location).Path 'sst.exe'
    if (Test-Path -LiteralPath $cwdCandidate -PathType Leaf) { return $cwdCandidate }

    $running = @(
        Get-Process -Name sst -ErrorAction SilentlyContinue | ForEach-Object {
            try {
                if ($_.Path -and (Test-Path -LiteralPath $_.Path -PathType Leaf)) { $_.Path }
            } catch {}
        } | Select-Object -Unique
    )
    if ($running.Count -eq 1) { return $running[0] }
    if ($running.Count -gt 1) {
        throw 'Hay varias instalaciones de SST en ejecución. Ejecuta este recovery desde la carpeta de la instalación que quieras actualizar.'
    }
    throw 'No se pudo localizar sst.exe. Ejecuta este recovery desde la carpeta donde está SST.'
}

$target = Resolve-SstTarget $TargetDir
$dir = Split-Path -Parent $target
$new = Join-Path $dir 'sst.exe.new'
$backup = Join-Path $dir 'sst.exe.old'

Write-Host "SST recovery: preparando $Version"
Write-Host "Destino: $target"

if (Test-Path -LiteralPath $new) { Remove-Item -Force -LiteralPath $new }
& "$env:SystemRoot\System32\curl.exe" -L --fail --silent --show-error --proto '=https' --tlsv1.2 --retry 3 --retry-all-errors --connect-timeout 10 --max-time 300 $DownloadUrl -o $new
if ($LASTEXITCODE -ne 0 -or -not (Test-Path -LiteralPath $new -PathType Leaf)) {
    throw 'No se pudo descargar sst.exe 0.1.9-blue.8.'
}

$actualSize = (Get-Item -LiteralPath $new).Length
$actualHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $new).Hash.ToLowerInvariant()
if ($actualSize -ne $ExpectedSize) {
    Remove-Item -Force -LiteralPath $new -ErrorAction SilentlyContinue
    throw "Tamaño incorrecto: $actualSize != $ExpectedSize"
}
if ($actualHash -ne $ExpectedSha256) {
    Remove-Item -Force -LiteralPath $new -ErrorAction SilentlyContinue
    throw "SHA-256 incorrecto: $actualHash"
}

$targetFull = [System.IO.Path]::GetFullPath($target)
$running = @(
    Get-Process -Name sst -ErrorAction SilentlyContinue | Where-Object {
        try { $_.Path -and [System.IO.Path]::GetFullPath($_.Path) -eq $targetFull } catch { $false }
    }
)

if ($running.Count -gt 0) {
    Write-Host 'Cerrando SST para aplicar la actualización...'
    foreach ($process in $running) {
        try { [void]$process.CloseMainWindow() } catch {}
    }

    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 250
        $stillRunning = @(
            Get-Process -Name sst -ErrorAction SilentlyContinue | Where-Object {
                try { $_.Path -and [System.IO.Path]::GetFullPath($_.Path) -eq $targetFull } catch { $false }
            }
        )
    } while ($stillRunning.Count -gt 0 -and [DateTime]::UtcNow -lt $deadline)

    if ($stillRunning.Count -gt 0) {
        throw 'SST no se cerró dentro de 30 segundos. No se forzó el cierre para evitar pérdida de trabajo. Cierra SST y vuelve a ejecutar el recovery.'
    }
}

if (Test-Path -LiteralPath $backup) { Remove-Item -Force -LiteralPath $backup }
Copy-Item -Force -LiteralPath $target -Destination $backup
Move-Item -Force -LiteralPath $new -Destination $target

$installedHash = (Get-FileHash -Algorithm SHA256 -LiteralPath $target).Hash.ToLowerInvariant()
if ($installedHash -ne $ExpectedSha256) {
    Copy-Item -Force -LiteralPath $backup -Destination $target
    throw 'La verificación posterior al reemplazo falló; se restauró sst.exe.old.'
}

Write-Host "SST actualizado a $Version."
Start-Process -FilePath $target -WorkingDirectory $dir
