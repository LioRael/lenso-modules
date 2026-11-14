#!/usr/bin/env bash
set -euo pipefail
if [ "$RUNNER_OS" = Linux ]; then
  docker run -d --name lenso-module-postgres -p 127.0.0.1:15432:5432 \
    -e POSTGRES_USER=lenso -e POSTGRES_PASSWORD=test-only-password -e POSTGRES_DB=modules postgres:18
  for attempt in $(seq 1 60); do
    if docker exec lenso-module-postgres pg_isready -h 127.0.0.1 -U lenso -d modules; then break; fi
    sleep 1
  done
  docker exec lenso-module-postgres pg_isready -h 127.0.0.1 -U lenso -d modules
else
  if ! brew list postgresql@18 >/dev/null 2>&1; then brew install postgresql@18; fi
  pg_bin="$(brew --prefix postgresql@18)/bin"
  pg_data="$RUNNER_TEMP/module-postgres"
  "$pg_bin/initdb" -D "$pg_data" -U lenso --auth=trust --no-locale
  "$pg_bin/pg_ctl" -D "$pg_data" -l "$RUNNER_TEMP/module-postgres.log" -o "-h 127.0.0.1 -p 15432 -k $RUNNER_TEMP" -w start
  "$pg_bin/createdb" -h 127.0.0.1 -p 15432 -U lenso modules
  echo "LENSO_TEST_PG_BIN=$pg_bin" >> "$GITHUB_ENV"
  echo "LENSO_TEST_PG_DATA=$pg_data" >> "$GITHUB_ENV"
fi
echo 'DATABASE_URL=postgresql://lenso:test-only-password@127.0.0.1:15432/modules?sslmode=disable' >> "$GITHUB_ENV"
echo 'AUTH_POSTGRES_URL=postgresql://lenso:test-only-password@127.0.0.1:15432/modules?sslmode=disable' >> "$GITHUB_ENV"
