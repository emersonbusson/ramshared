try {
    Invoke-WebRequest -Uri "https://www.google.com" -ErrorAction Stop | Out-Null
    Write-Host "Success"
} catch {
    Write-Host "Failed: $($_.Exception.Message)"
}
