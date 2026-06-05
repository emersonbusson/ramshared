<#
.SYNOPSIS
  Troca o kernel do WSL2 de forma SEGURA e AUTO-CURÁVEL.

.DESCRIPTION
  Arma o kernel custom no .wslconfig (com backup), reinicia o WSL e verifica o boot
  com timeout. Se o kernel NÃO bootar (timeout ou versão errada), RESTAURA o .wslconfig
  do backup e reinicia → volta sozinho ao kernel da Microsoft. "Se der problema, arruma
  sozinho." Reutilizável p/ qualquer kernel custom (toolkit Fase B+).

  Critério de auto-revert = FALHA DE BOOT (catastrófico). Se o kernel boota mas um
  módulo (ex.: ublk_drv) falha, NÃO reverte (kernel é usável) — só avisa.

.PARAMETER KernelPath     Caminho Windows do bzImage (default C:\wsl\kernel-ramshared)
.PARAMETER ExpectedVersion `uname -r` esperado (default 6.6.123.2-microsoft-standard-WSL2+)
.PARAMETER WslConfig      .wslconfig (default $env:USERPROFILE\.wslconfig)
.PARAMETER CleanConfig    .wslconfig limpo p/ restaurar no fail (default C:\wsl\wslconfig-original.txt)
.PARAMETER TimeoutSec     Timeout do boot-check (default 60)
.PARAMETER CheckModules   Módulos a testar pós-boot via modprobe (default "ublk_drv")
.PARAMETER DryRunConfig   Se setado: só exercita a lógica de Arm/Revert nesse arquivo e sai (teste, não toca o WSL)
#>
param(
  [string]$KernelPath      = "C:\wsl\kernel-ramshared",
  [string]$ExpectedVersion = "6.6.123.2-microsoft-standard-WSL2+",
  [string]$WslConfig       = "$env:USERPROFILE\.wslconfig",
  [string]$CleanConfig     = "C:\wsl\wslconfig-original.txt",
  [int]   $TimeoutSec      = 60,
  [string]$CheckModules    = "ublk_drv",
  [string]$DryRunConfig    = ""
)
$ErrorActionPreference = "Stop"

# Caminho do kernel no formato .wslconfig (backslash duplo, estilo do arquivo existente).
function To-WslPath([string]$p) { return ($p -replace '\\','\\') }

# Arma kernel= sob [wsl2] de forma idempotente (substitui se já existir; cria [wsl2] se faltar).
function Arm-Config([string]$cfgPath, [string]$kernelWin) {
  $kline = "kernel=" + (To-WslPath $kernelWin)
  $lines = @(); if (Test-Path $cfgPath) { $lines = @(Get-Content $cfgPath) }
  $out = @(); $inWsl2 = $false; $added = $false; $hasWsl2 = $false
  foreach ($l in $lines) {
    if ($l -match '^\s*\[wsl2\]\s*$')      { $inWsl2 = $true; $hasWsl2 = $true; $out += $l; continue }
    if ($l -match '^\s*\[')                { if ($inWsl2 -and -not $added) { $out += $kline; $added = $true }; $inWsl2 = $false; $out += $l; continue }
    if ($inWsl2 -and $l -match '^\s*kernel\s*=') { continue }  # remove kernel= antigo (substitui)
    $out += $l
  }
  if ($inWsl2 -and -not $added) { $out += $kline; $added = $true }          # [wsl2] era a última seção
  if (-not $hasWsl2)            { $out = @("[wsl2]", $kline) + $out }        # não havia [wsl2]
  if (-not (Set-CfgRetry $cfgPath $out)) { throw "não consegui escrever $cfgPath (locked/ACL?)" }
}

# Escrita com retry — locks transitórios do .wslconfig (WSL/editor/antivírus/OneDrive).
function Set-CfgRetry([string]$path, [string[]]$lines) {
  for ($i = 0; $i -lt 6; $i++) {
    try { Set-Content -Path $path -Value $lines -Encoding ASCII -ErrorAction Stop; return $true }
    catch { Start-Sleep -Milliseconds 800 }
  }
  return $false
}

# Desarme DETERMINÍSTICO: remove TODA linha kernel= (volta ao kernel da Microsoft).
# NÃO depende de o backup conseguir ser copiado. Retorna $true se desarmou (ou se não há config).
# Nunca lança (captura tudo) — é chamado do finally, onde uma exceção deixaria armado-quebrado.
function Disarm-Config([string]$cfgPath) {
  try {
    if (-not (Test-Path $cfgPath)) { return $true }
    $kept = @(Get-Content $cfgPath | Where-Object { $_ -notmatch '^\s*kernel\s*=' })
    return (Set-CfgRetry $cfgPath $kept)
  } catch { return $false }
}

# --- Modo TESTE (não toca o WSL): exercita Arm + mostra o resultado ---
if ($DryRunConfig -ne "") {
  Write-Host "[dry-run] armando kernel em $DryRunConfig ..."
  Arm-Config $DryRunConfig $KernelPath
  Write-Host "--- resultado ---"; Get-Content $DryRunConfig | ForEach-Object { Write-Host $_ }
  Write-Host "[dry-run] (idempotência) armando de novo ..."
  Arm-Config $DryRunConfig $KernelPath
  $n = @(Select-String -Path $DryRunConfig -Pattern '^\s*kernel=').Count
  Write-Host "ARM: linhas kernel= = $n (esperado 1)"
  Write-Host "[dry-run] desarmando (revert determinístico) ..."
  $ok = Disarm-Config $DryRunConfig
  $d = @(Select-String -Path $DryRunConfig -Pattern '^\s*kernel=').Count
  Write-Host "DISARM: ok=$ok ; linhas kernel= = $d (esperado 0)"
  exit 0
}

# --- 1. backup LIMPO garantido (revert SEMPRE restaura algo bootável) ---
if (Test-Path $CleanConfig) {
  if (Select-String -Path $CleanConfig -Pattern '^\s*kernel=' -Quiet) {
    Write-Error "backup '$CleanConfig' NÃO está limpo (contém kernel=). Aponte -CleanConfig p/ um .wslconfig SEM kernel custom."
  }
} else {
  if ((Test-Path $WslConfig) -and (Select-String -Path $WslConfig -Pattern '^\s*kernel=' -Quiet)) {
    Write-Error "Sem backup limpo e o .wslconfig atual já tem kernel=. Crie '$CleanConfig' (versão sem kernel=) antes."
  }
  if (Test-Path $WslConfig) { Copy-Item $WslConfig $CleanConfig -Force } else { Set-Content $CleanConfig "[wsl2]" -Encoding ASCII }
  Write-Host "backup limpo criado: $CleanConfig"
}

# --- 2-4. arma + reinicia + verifica, com FAIL-SAFE total: QUALQUER falha/erro/exceção
# (inclusive wsl --shutdown lançar) → o finally reverte ao kernel MS. Nunca fica armado-quebrado.
$confirmed = $false; $uname = ""
try {
  Write-Host "armando kernel=$KernelPath em $WslConfig (backup: $CleanConfig)"
  Arm-Config $WslConfig $KernelPath
  Write-Host "wsl --shutdown ..."; wsl --shutdown; Start-Sleep -Seconds 3
  Write-Host "bootando + verificando (timeout ${TimeoutSec}s)..."
  $job = Start-Job -ScriptBlock { (wsl.exe -e sh -c "uname -r") 2>&1 }
  if (Wait-Job $job -Timeout $TimeoutSec) {
    $uname = ((Receive-Job $job) -join "`n").Trim()
    if ($uname -match [regex]::Escape($ExpectedVersion)) { $confirmed = $true }
    else { Write-Warning "uname inesperado: '$uname' (esperado conter '$ExpectedVersion')" }
  } else {
    Stop-Job $job; Write-Warning "boot NÃO respondeu em ${TimeoutSec}s (provável falha de boot)"
  }
  Remove-Job $job -Force -ErrorAction SilentlyContinue
} catch {
  Write-Warning "erro durante a troca: $_"
} finally {
  if (-not $confirmed) {
    Write-Warning "FALHA → auto-revertendo ao kernel da Microsoft..."
    # Desarme determinístico (não depende do backup) + nunca escapa do finally.
    if (Disarm-Config $WslConfig) {
      try { wsl --shutdown } catch { }
      Write-Host "REVERTIDO. O próximo wsl usa o kernel da Microsoft. Nenhum dado afetado."
    } else {
      Write-Warning ("REVERT AUTOMÁTICO FALHOU ao reescrever $WslConfig (locked/ACL?). " +
        "AÇÃO MANUAL: apague a linha 'kernel=' de $WslConfig e rode 'wsl --shutdown'. Backup limpo: $CleanConfig")
    }
  }
}

# --- 5. resultado (módulo é best-effort: NÃO reverte; kernel é usável mesmo se falhar) ---
if ($confirmed) {
  Write-Host "OK: kernel custom bootou ($uname)."
  $mod = (wsl.exe -e sh -c "sudo modprobe $CheckModules 2>&1 && ls /dev/ublk-control 2>/dev/null && echo MOD-OK") 2>&1
  if ($mod -match "MOD-OK") { Write-Host "módulos OK ($CheckModules carregou)." }
  else { Write-Warning "kernel OK, mas módulo '$CheckModules' não carregou: $mod (kernel mantido; investigar)" }
  Write-Host "PRONTO. Kernel custom ativo."
  exit 0
} else {
  exit 1
}
