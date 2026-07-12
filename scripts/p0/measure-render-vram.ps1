# P0 ITEM-1 — medição de VRAM/RAM num render do Blender (script para o tester, Windows/PowerShell).
# Feeds the P2 GATE (native out-of-core): proves, with NUMBERS, the scene that failed due to VRAM.
# Does NOT modify the scene (Annex B.5 of PRD) — only monitors nvidia-smi + available RAM and reads the render log.
# SPEC: docs/memory-broker/SPECv2.md ITEM-1. Regra de medição: .claude/rules/benchmarks.md.
#
# DOIS MODOS:
#   Full   (recomendado): passa -Blend <cena.blend> → o script LANÇA o Blender headless, amostra a
#          VRAM/RAM durante o render, captura o exit code + a MENSAGEM DE ERRO exata do log, e roda
#          -Runs vezes (mediana + p99 + desvio, como manda o benchmarks.md).
#   Passive (legacy): without -Blend -> only samples for -DurationSec while YOU render manually; in this
#          mode, also provide a screenshot of the error message (the script cannot see the Blender log).
#
# Uso (full):    .\measure-render-vram.ps1 -Blend C:\cenas\quebra.blend -Runs 3 -Tag idle
# Uso (passivo): .\measure-render-vram.ps1 -DurationSec 600
# Se o PowerShell bloquear o .ps1:
#   powershell -ExecutionPolicy Bypass -File .\measure-render-vram.ps1 -Blend C:\cenas\quebra.blend
param(
    [string]$Blend,                                  # cena que falha; vazio => modo passivo
    [string]$BlenderExe = "blender",                 # 'blender' no PATH ou caminho completo do .exe
    [int]$Runs = 3,                                  # >=3 (benchmarks.md); mediana+p99+desvio
    [int]$IntervalMs = 500,                          # período de amostragem
    [int]$Frame = 1,                                 # frame a renderizar (-f)
    [ValidateSet("idle", "loaded")][string]$Tag = "idle",  # condição da máquina (benchmarks.md)
    [int]$GpuIndex = 0,                              # índice da GPU no nvidia-smi
    [int]$DurationSec = 600,                         # só no modo passivo
    [int]$MaxRunSec = 3600,                          # trava de segurança por run (evita travar eterno)
    [string]$OutDir = "ramshared-p0-$(Get-Date -Format yyyyMMdd-HHmmss)"
)
$ErrorActionPreference = "Stop"
function Log($m) { Write-Host "[p0-render] $m" }

# --- preflight -------------------------------------------------------------
if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) {
    throw "nvidia-smi ausente (driver NVIDIA instalado?). Sem ele não há como medir VRAM."
}
$mode = if ($Blend) { "full" } else { "passive" }

function Resolve-Blender($exe) {
    $c = Get-Command $exe -ErrorAction SilentlyContinue
    if ($c) { return $c.Source }
    $cands = Get-ChildItem "C:\Program Files\Blender Foundation\*\blender.exe" -ErrorAction SilentlyContinue |
        Sort-Object FullName -Descending
    if ($cands) { return $cands[0].FullName }
    return $null
}

$blenderResolved = $null
if ($mode -eq "full") {
    if (-not (Test-Path $Blend)) { throw "cena não encontrada: $Blend" }
    $blenderResolved = Resolve-Blender $BlenderExe
    if (-not $blenderResolved) {
        throw "Blender não encontrado ('$BlenderExe'). Passe -BlenderExe 'C:\Program Files\Blender Foundation\Blender X.Y\blender.exe'."
    }
}

New-Item -ItemType Directory -Force -Path $OutDir | Out-Null

# --- captura de contexto (benchmarks.md: o contexto É dado) -----------------
$os = Get-CimInstance Win32_OperatingSystem
$ramTotalMib = [int]($os.TotalVisibleMemorySize / 1024)   # KB -> MiB
$gpuCsv = (& nvidia-smi --query-gpu=name,memory.total,driver_version --format=csv,noheader,nounits -i $GpuIndex) -split ','
$gpuName = $gpuCsv[0].Trim(); $gpuVramTotal = [int]$gpuCsv[1].Trim(); $driver = $gpuCsv[2].Trim()
$blenderVer = $null
if ($blenderResolved) { $blenderVer = (& $blenderResolved --version 2>$null | Select-Object -First 1) }

# Pré-computado (evita `if`-como-expressão dentro de literal de hashtable, que não parseia em PS).
$blendLeaf = $null
if ($Blend) { $blendLeaf = Split-Path $Blend -Leaf }

$ctx = [ordered]@{
    tool = "measure-render-vram.ps1"; mode = $mode; tag = $Tag
    os = $os.Caption; os_version = $os.Version; ram_total_mib = $ramTotalMib
    gpu = $gpuName; gpu_vram_mib = $gpuVramTotal; driver = $driver
    blender = $blenderVer; blend = $blendLeaf
    frame = $Frame; runs = $Runs; interval_ms = $IntervalMs
}
($ctx | ConvertTo-Json) | Out-File -Encoding utf8 (Join-Path $OutDir "context.json")
Log "contexto: $gpuName ($gpuVramTotal MiB) | $($os.Caption) | RAM $ramTotalMib MiB | Blender: $blenderVer"

# --- helpers de amostragem e estatística ------------------------------------
function Sample-VramUsed($idx) {
    $l = & nvidia-smi --query-gpu=memory.used --format=csv,noheader,nounits -i $idx 2>$null
    if ($l) { return [int]($l.Trim()) } else { return $null }
}
function Sample-RamFreeMib { [int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1024) }
function Pctile($vals, $p) {
    $s = @($vals | Sort-Object); if ($s.Count -eq 0) { return $null }
    $rank = [int]([math]::Ceiling($p / 100.0 * $s.Count)) - 1
    if ($rank -lt 0) { $rank = 0 } elseif ($rank -ge $s.Count) { $rank = $s.Count - 1 }
    return $s[$rank]
}
function Stddev($vals) {
    $a = @($vals); if ($a.Count -lt 2) { return 0 }
    $m = ($a | Measure-Object -Average).Average
    $var = ($a | ForEach-Object { ($_ - $m) * ($_ - $m) } | Measure-Object -Sum).Sum / ($a.Count - 1)
    return [math]::Round([math]::Sqrt($var), 1)
}
# Padrões de OOM/erro do Cycles (CUDA e OptiX). Case-insensitive.
$OOM = @('out of memory', 'CUDA error', 'OPTIX_ERROR', 'failed to allocate', 'out of GPU memory', 'System is out of')
function Scan-Log($paths) {
    foreach ($p in $paths) {
        if (Test-Path $p) {
            $hit = Select-String -Path $p -Pattern $OOM -List -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($hit) { return $hit.Line.Trim() }
        }
    }
    return $null
}
function Detect-Backend($paths) {
    foreach ($p in $paths) {
        if (Test-Path $p) {
            foreach ($k in @('OptiX', 'CUDA', 'HIP', 'oneAPI', 'Metal')) {
                if (Select-String -Path $p -Pattern $k -SimpleMatch -Quiet -ErrorAction SilentlyContinue) { return $k }
            }
        }
    }
    return $null
}

# --- uma rodada -------------------------------------------------------------
function Invoke-Run($i) {
    $csv = Join-Path $OutDir ("run{0}.csv" -f $i)
    "ts_ms,vram_used_mib,ram_free_mib" | Out-File -Encoding ascii $csv
    $tsStart = [int64]([datetimeoffset]::UtcNow.ToUnixTimeSeconds())
    $peak = 0; $minRam = [int]::MaxValue; $n = 0
    $exit = $null; $errLine = $null; $backend = $null
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    if ($mode -eq "full") {
        $outLog = Join-Path $OutDir ("run{0}.out.log" -f $i)
        $errLog = Join-Path $OutDir ("run{0}.err.log" -f $i)
        Log ("run {0}/{1}: lançando Blender headless (-b -f {2})..." -f $i, $Runs, $Frame)
        $proc = Start-Process -FilePath $blenderResolved `
            -ArgumentList @('-b', $Blend, '-f', $Frame) `
            -NoNewWindow -PassThru -RedirectStandardOutput $outLog -RedirectStandardError $errLog
        while (-not $proc.HasExited) {
            $v = Sample-VramUsed $GpuIndex; $r = Sample-RamFreeMib
            $ts = [int64]([datetimeoffset]::UtcNow.ToUnixTimeMilliseconds())
            "$ts,$v,$r" | Out-File -Encoding ascii -Append $csv
            if ($v -ne $null -and $v -gt $peak) { $peak = $v }
            if ($r -lt $minRam) { $minRam = $r }
            $n++
            if ($sw.Elapsed.TotalSeconds -gt $MaxRunSec) { Log "MaxRunSec estourado; matando render"; $proc.Kill(); break }
            Start-Sleep -Milliseconds $IntervalMs
        }
        $proc.WaitForExit()
        $exit = $proc.ExitCode
        $errLine = Scan-Log @($errLog, $outLog)
        $backend = Detect-Backend @($outLog, $errLog)
        $failed = ($exit -ne 0) -or ($errLine -ne $null)
        Log ("run {0}: exit={1} failed={2} pico_VRAM={3} MiB min_RAM_livre={4} MiB backend={5}" -f $i, $exit, $failed, $peak, $minRam, $backend)
        if ($errLine) { Log ("  erro: {0}" -f $errLine) }
    }
    else {
        Log ("run {0}/{1} (PASSIVO): INICIE o render AGORA; amostrando por {2}s (não altere a cena)." -f $i, $Runs, $DurationSec)
        while ($sw.Elapsed.TotalSeconds -lt $DurationSec) {
            $v = Sample-VramUsed $GpuIndex; $r = Sample-RamFreeMib
            $ts = [int64]([datetimeoffset]::UtcNow.ToUnixTimeMilliseconds())
            "$ts,$v,$r" | Out-File -Encoding ascii -Append $csv
            if ($v -ne $null -and $v -gt $peak) { $peak = $v }
            if ($r -lt $minRam) { $minRam = $r }
            $n++
            Start-Sleep -Milliseconds $IntervalMs
        }
        $failed = $null  # desconhecido no modo passivo
    }
    $dur = [math]::Round($sw.Elapsed.TotalSeconds, 1)
    if ($minRam -eq [int]::MaxValue) { $minRam = $null }
    # Pré-computado (evita `if`-como-expressão dentro do literal de hashtable).
    $ramUsedPeak = $null
    if ($minRam -ne $null) { $ramUsedPeak = $ramTotalMib - $minRam }

    $rec = [ordered]@{
        run = $i; tag = $Tag; mode = $mode; ts_start = $tsStart
        os = $os.Caption; os_version = $os.Version; ram_total_mib = $ramTotalMib
        gpu = $gpuName; gpu_vram_mib = $gpuVramTotal; driver = $driver
        blender = $blenderVer; backend = $backend
        blend = $blendLeaf; frame = $Frame
        peak_vram_used_mib = $peak; vram_total_mib = $gpuVramTotal
        min_ram_free_mib = $minRam; ram_used_peak_mib = $ramUsedPeak
        samples = $n; duration_s = $dur; exit_code = $exit; failed = $failed; error_line = $errLine
    }
    ($rec | ConvertTo-Json -Compress) | Add-Content -Encoding ascii (Join-Path $OutDir "results.jsonl")
    return $rec
}

# --- loop de runs + agregação ----------------------------------------------
$recs = @()
for ($i = 1; $i -le $Runs; $i++) { $recs += (Invoke-Run $i) }

$peaks = @($recs | ForEach-Object { $_.peak_vram_used_mib })
$median = Pctile $peaks 50
$p99 = Pctile $peaks 99
$sd = Stddev $peaks
$nFail = @($recs | Where-Object { $_.failed -eq $true }).Count

$md = @()
$md += "## Render VRAM/RAM — $gpuName ($gpuVramTotal MiB) — tag=$Tag — $(Get-Date -Format s)"
$md += ""
$md += "- **Contexto:** $($os.Caption) | RAM $ramTotalMib MiB | driver $driver | Blender $blenderVer | backend $($recs[0].backend)"
$md += "- **Cena:** $($ctx.blend) (frame $Frame) | modo=$mode | runs=$Runs"
$md += "- **Pico VRAM usada (MiB):** mediana **$median** | p99 $p99 | desvio $sd (de: $($peaks -join ', '))"
$md += "- **Falhou (OOM/exit!=0):** $nFail de $Runs run(s)"
$md += "- **RAM livre mínima (MiB):** $(($recs | ForEach-Object { $_.min_ram_free_mib }) -join ', ')"
if ($recs[0].error_line) { $md += "- **Erro capturado:** ``$($recs[0].error_line)``" }
$md += ""
$md += "Arquivos: ``$OutDir\{context.json, results.jsonl, run*.csv$(if($mode -eq 'full'){', run*.out.log, run*.err.log'})}``"
$md | Out-File -Encoding utf8 (Join-Path $OutDir "summary.md")

Log ""
Log "=== RESUMO ==="
$md | ForEach-Object { Write-Host $_ }
Log ""
Log "PRONTO. Compacte a pasta e envie:  Compress-Archive -Path '$OutDir\*' -DestinationPath '$OutDir.zip'"
if ($mode -eq "full") {
    Log "Envie tambem a cena .blend (Anexo B.3). O erro exato ja foi capturado nos *.err.log/results.jsonl."
}
else {
    Log "Modo passivo: envie tambem a cena .blend + um PRINT da mensagem de erro do Blender (Anexo B.3)."
}
