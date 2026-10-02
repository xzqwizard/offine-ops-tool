#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
command -v docker >/dev/null
docker info >/dev/null
command -v iptables-restore >/dev/null
work=$(mktemp -d /tmp/preops-acceptance.XXXXXX)
network="preops-acceptance-$$"
trap 'docker network rm "$network" >/dev/null 2>&1 || true; sudo rm -rf -- "$work"' EXIT
docker pull docker.io/library/nginx:1.27.2
docker pull docker.io/library/postgres:16.4
docker pull curlimages/curl:8.11.1
docker save -o "$work/nginx.tar" docker.io/library/nginx:1.27.2
docker save -o "$work/postgres.tar" docker.io/library/postgres:16.4
docker network create --subnet 172.30.252.0/24 "$network" >/dev/null
host_ip=$(docker network inspect bridge --format '{{(index .IPAM.Config 0).Gateway}}')
docker_version=$(docker --version | awk '{print $3}' | tr -d ',')
docker_root=$(docker info --format '{{.DockerRootDir}}')
port=18080
render() {
  cargo run --quiet --locked --manifest-path "$ROOT/src-tauri/Cargo.toml" --example render_acceptance -- \
    "$work/$1" "$work/nginx.tar" "$host_ip" "$docker_version" "$docker_root" "$2" "$3" "$4" "$work/postgres.tar"
}
sql() {
  docker compose --project-directory "$work/full/stack" -f "$work/full/stack/docker-compose.yml" \
    exec -T db sh -c 'PGPASSWORD="$POSTGRES_PASSWORD" psql -h127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -tAc "$1"' sh "$1"
}
render full b-one '' "$port"
sudo bash "$work/full/scripts/deploy.sh"
sudo bash "$work/full/scripts/deploy.sh"
sudo bash "$work/full/scripts/ops.sh" status
curl --fail --silent "http://$host_ip:$port/" >/dev/null
sql "CREATE TABLE acceptance (value text NOT NULL); INSERT INTO acceptance VALUES ('before');" >/dev/null
sudo bash "$work/full/scripts/apply-firewall.sh" --apply --no-persist
docker run --rm --network "$network" --ip 172.30.252.10 curlimages/curl:8.11.1 -fsS --connect-timeout 5 "http://$host_ip:$port/" >/dev/null
if docker run --rm --network "$network" --ip 172.30.252.11 curlimages/curl:8.11.1 -fsS --connect-timeout 3 "http://$host_ip:$port/" >/dev/null 2>&1; then
  echo 'Denied source reached published port' >&2; exit 1
fi
sudo bash "$work/full/scripts/ops.sh" backup
snapshot=$(sudo find "$work/backups/proj-acceptance/srv-acceptance" -mindepth 1 -maxdepth 1 -type d | head -n 1)
sudo test -f "$snapshot/COMPLETE"
sql "INSERT INTO acceptance VALUES ('after');" >/dev/null
sudo sh -c "printf changed > '$work/full/stack/data/web/index.html'"
sudo bash "$work/full/scripts/ops.sh" restore "$snapshot" --yes
test "$(sudo cat "$work/full/stack/data/web/index.html" | tr -d '\n')" != changed
test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
render upgrade b-two b-one 18081
sudo bash "$work/upgrade/scripts/upgrade.sh" --target "$work/full" --dry-run
sudo bash "$work/upgrade/scripts/upgrade.sh" --target "$work/full"
sudo bash "$work/full/scripts/ops.sh" status
test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
test "$(sudo sed -n "s/^BUILD_ID='\([^']*\)'/\1/p" "$work/full/deployment.env")" = b-two
if sudo bash "$work/full/scripts/deploy.sh"; then echo 'Stale full deploy reran after upgrade' >&2; exit 1; fi
render fail b-three b-two 18082
sudo sed -i 's#docker.io/library/nginx:1.27.2#unavailable:image#g' "$work/fail/stack/docker-compose.yml"
(cd "$work/fail" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
if sudo bash "$work/fail/scripts/upgrade.sh" --target "$work/full"; then echo 'Broken upgrade succeeded' >&2; exit 1; fi
sudo bash "$work/full/scripts/ops.sh" status
test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
test "$(sudo sed -n "s/^BUILD_ID='\([^']*\)'/\1/p" "$work/full/deployment.env")" = b-two
sudo bash "$work/full/scripts/ops.sh" rollback "$snapshot" --yes
sudo bash "$work/full/scripts/ops.sh" status
test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
test "$(sudo sed -n "s/^BUILD_ID='\([^']*\)'/\1/p" "$work/full/deployment.env")" = b-one
echo 'Linux Docker deployment, firewall, PostgreSQL backup/restore, upgrade and rollback passed'
