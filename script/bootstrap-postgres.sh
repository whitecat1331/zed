#!/usr/bin/env bash
set -euo pipefail

# Bootstraps a local PostgreSQL instance for Zed's agent thread storage.
#
# Creates a `zed_threads` database reachable at
# `postgres://localhost/zed_threads` (Zed's default). On Linux this installs
# PostgreSQL via the system package manager and uses the `postgres` superuser;
# on macOS it installs via Homebrew.
#
# This script is intentionally not run automatically. Run it once, then either
# keep Zed's default URL or point Zed at another database via the
# `agent.threads_database_url` setting or the `ZED_THREADS_DATABASE_URL`
# environment variable.

DB_NAME="${ZED_THREADS_DB_NAME:-zed_threads}"

if [[ "$(uname -s)" == "Darwin" ]]; then
    if ! command -v brew >/dev/null 2>&1; then
        echo "error: Homebrew is required to bootstrap PostgreSQL on macOS" >&2
        exit 1
    fi
    echo "Installing PostgreSQL via Homebrew..."
    brew install postgresql@16
    brew services start postgresql@16

    # postgresql@16 is keg-only; its binaries live in its prefix.
    PSQL_BIN="$(brew --prefix postgresql@16)/bin"
    if ! "$PSQL_BIN/psql" -d postgres -tAc "SELECT 1 FROM pg_database WHERE datname='${DB_NAME}'" | grep -q 1; then
        "$PSQL_BIN/createdb" "${DB_NAME}"
    fi
    echo "PostgreSQL is ready. Zed will use: postgres://localhost/${DB_NAME}"
    exit 0
fi

if command -v apt-get >/dev/null 2>&1; then
    echo "Installing PostgreSQL via apt..."
    sudo apt-get update
    sudo apt-get install -y postgresql
elif command -v dnf >/dev/null 2>&1; then
    echo "Installing PostgreSQL via dnf..."
    sudo dnf install -y postgresql-server postgresql
    sudo postgresql-setup --initdb
    sudo systemctl enable --now postgresql
elif command -v pacman >/dev/null 2>&1; then
    echo "Installing PostgreSQL via pacman..."
    sudo pacman -S --noconfirm postgresql
    sudo -u postgres initdb -D /var/lib/postgres/data 2>/dev/null || true
    sudo systemctl enable --now postgresql
else
    echo "error: unsupported Linux package manager (expected apt, dnf, or pacman)" >&2
    exit 1
fi

if command -v systemctl >/dev/null 2>&1; then
    sudo systemctl enable --now postgresql 2>/dev/null || true
fi

if ! sudo -u postgres psql -tAc "SELECT 1 FROM pg_database WHERE datname='${DB_NAME}'" | grep -q 1; then
    sudo -u postgres createdb "${DB_NAME}"
fi

echo "PostgreSQL is ready. Zed will use: postgres://localhost/${DB_NAME}"
