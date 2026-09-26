param(
    [string]$Exe = ".\target\debug\adm-toolbox.exe"
)

$ErrorActionPreference = "Stop"

if (-not (Test-Path $Exe)) {
    throw "No existe $Exe. Ejecuta cargo build primero."
}

$cases = @(
    @{ Name="quotes"; Script='x="a b"; printf "%s\n" "$x"' },
    @{ Name="parameter-default"; Script='unset x; printf "%s\n" "${x:-fallback}"' },
    @{ Name="arithmetic"; Script='x=20; printf "%s\n" "$((x+22))"' },
    @{ Name="function"; Script='f() { printf "f=%s\n" "$1"; }; f hello' },
    @{ Name="if"; Script='if true; then echo yes; else echo no; fi' },
    @{ Name="for"; Script='for x in a b c; do echo "$x"; done' },
    @{ Name="pipeline"; Script='printf "c\na\nb\n" | sort' },
    @{ Name="subshell"; Script='x=before; (x=inside; echo "$x"); echo "$x"' },
    @{ Name="command-substitution"; Script='x=$(printf hello); echo "$x"' },
    @{ Name="logical"; Script='false || echo recovered; true && echo ok' }
)

$failed = 0

foreach ($case in $cases) {
    $ours = & $Exe -c $case.Script 2>&1 | Out-String
    $oursExit = $LASTEXITCODE

    $bash = wsl.exe bash -c $case.Script 2>&1 | Out-String
    $bashExit = $LASTEXITCODE

    if ($ours -ceq $bash -and $oursExit -eq $bashExit) {
        Write-Host "PASS $($case.Name)"
    }
    else {
        $failed++
        Write-Host "FAIL $($case.Name)" -ForegroundColor Red
        Write-Host "  ADM exit=$oursExit output=[$ours]"
        Write-Host "  WSL exit=$bashExit output=[$bash]"
    }
}

if ($failed -gt 0) {
    throw "$failed prueba(s) de conformidad fallaron"
}

Write-Host "Conformidad básica: OK"
