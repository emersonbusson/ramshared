# P0 ITEM-1 — poll de VRAM/RAM durante um render (script para o tester, Windows/PowerShell).
# NAO altera a cena (Anexo B.5 do PRD): so observa nvidia-smi + contador de RAM disponivel.
# Uso: .\measure-render-vram.ps1 [-DurationSec 600] [-Out vram.csv]
# SPEC: docs/memory-broker/SPECv2.md ITEM-1 — alimenta o gate de P2 (out-of-core nativo).
param(
	[int]$DurationSec = 600,
	[string]$Out = "vram-$(Get-Date -Format yyyyMMdd-HHmmss).csv"
)
$ErrorActionPreference = "Stop"
function Log($m) { Write-Host "[p0-render] $m" }

if (-not (Get-Command nvidia-smi -ErrorAction SilentlyContinue)) {
	throw "nvidia-smi ausente (driver NVIDIA instalado?)"
}

Log "amostrando VRAM/RAM por ${DurationSec}s -> $Out"
Log "INICIE o render AGORA; nao altere a cena durante a coleta."
# RAM livre via CIM (Win32_OperatingSystem.FreePhysicalMemory, em KB) — NEUTRO de locale.
# O contador '\Memory\Available MBytes' do Get-Counter é localizado (quebra em Windows pt-BR etc.).
"ts,vram_used_mib,vram_total_mib,ram_free_mib" | Out-File -Encoding ascii $Out

$end = (Get-Date).AddSeconds($DurationSec)
while ((Get-Date) -lt $end) {
	$ts  = [int][double]::Parse((Get-Date -UFormat %s))
	$gpu = (nvidia-smi --query-gpu=memory.used,memory.total --format=csv,noheader,nounits) `
		-split ',' | ForEach-Object { $_.Trim() }
	$ram = [int]((Get-CimInstance Win32_OperatingSystem).FreePhysicalMemory / 1024)
	"$ts,$($gpu[0]),$($gpu[1]),$ram" | Out-File -Encoding ascii -Append $Out
	Start-Sleep -Seconds 1
}

Log "fim: $((Get-Content $Out).Count - 1) amostras em $Out"
Log "envie o CSV + a cena/.blend e a mensagem de erro exata (Anexo B do PRD)"
