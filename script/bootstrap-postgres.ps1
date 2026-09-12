# Bootstraps a local PostgreSQL instance for Zed's agent thread storage on Windows.
#
# Installs PostgreSQL 16 via winget, then creates a `zed_threads` database
# reachable at `postgres://localhost/zed_threads` (Zed's default).
#
# This script is intentionally not run automatically. Run it once from an
# elevated PowerShell, then either keep Zed's default URL or point Zed at another
# database via the `agent.threads_database_url` setting or the
# `ZED_THREADS_DATABASE_URL` environment variable.

$ErrorActionPreference = "Stop"

$DbName = if ($env:ZED_THREADS_DB_NAME) { $env:ZED_THREADS_DB_NAME } else { "zed_threads" }

if (-not (Get-Command winget -ErrorAction SilentlyContinue)) {
    Write-Error "winget is required to bootstrap PostgreSQL on Windows. Install it from the Microsoft Store or https://aka.ms/getwinget"
    exit 1
}

Write-Host "Installing PostgreSQL 16 via winget..."
winget install --id PostgreSQL.PostgreSQL.16 --accept-source-agreements --accept-package-agreements

# The PostgreSQL 16 installer places psql/createdb on the PATH under Program Files.
$Candidates = @(
    "C:\Program Files\PostgreSQL\16\bin",
    "C:\Program Files\PostgreSQL\16\bin\psql.exe"
)
$Psql = $null
foreach ($Path in $Candidates) {
    if (Test-Path $Path) {
        $Psql = "C:\Program Files\PostgreSQL\16\bin\psql.exe"
        break
    }
}
if (-not $Psql) {
    Write-Error "Could not locate psql.exe. Install PostgreSQL 16 manually, then run: createdb $DbName"
    exit 1
}

$Createdb = Join-Path (Split-Path $Psql) "createdb.exe"

$env:PGPASSWORD = "postgres"
$Exists = & $Psql -U postgres -h localhost -tAc "SELECT 1 FROM pg_database WHERE datname='$DbName'" 2>$null
if ($Exists -notmatch "1") {
    & $Createdb -U postgres -h localhost $DbName
    if ($LASTEXITCODE -ne 0) {
        Write-Error "Failed to create database '$DbName'. Run 'createdb -U postgres -h localhost $DbName' manually."
        exit 1
    }
}
Remove-Item Env:PGPASSWORD -ErrorAction SilentlyContinue

Write-Host "PostgreSQL is ready. Zed will use: postgres://localhost/$DbName"
