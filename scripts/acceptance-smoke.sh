#!/usr/bin/env bash
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
SCENARIO=${1:-all}
case "$SCENARIO" in all|network|operations|identity) ;; *) echo 'usage: acceptance-smoke.sh [network|operations|identity|all]' >&2; exit 1;; esac
if [ "$SCENARIO" = all ]; then
  result=0
  for scenario in network operations identity; do bash "$0" "$scenario" || result=1; done
  exit "$result"
fi
command -v docker >/dev/null; docker info >/dev/null
command -v iptables-restore >/dev/null
as_root() { if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo --preserve-env=DOCKER_HOST,DOCKER_CONFIG "$@"; fi; }
work=$(mktemp -d /tmp/preops-acceptance.XXXXXX)
if [ -n "${PREOPS_RENDERER:-}" ]; then chmod 777 "$work"; fi
network="preops-acceptance-$$"
host_ip=''; chain=''
cleanup() {
  local rc=$?; trap - EXIT
  if [ "$rc" -ne 0 ]; then
    docker version || true; docker info || true
    as_root iptables-save -c || true
    as_root ss -lntup || true
    docker ps -a || true
    if [ -f "$work/full/stack/docker-compose.yml" ]; then docker compose -f "$work/full/stack/docker-compose.yml" logs --tail 100 || true; fi
    echo "Failed $SCENARIO acceptance; diagnostics above" >&2
  fi
  docker ps -aq --filter label=offlinepreops.project=proj-acceptance | xargs -r docker rm -f >/dev/null 2>&1 || true
  docker network rm "$network" >/dev/null 2>&1 || true
  if [ -n "$chain" ]; then
    as_root iptables -D DOCKER-USER -m conntrack --ctorigdst "$host_ip" -j "$chain" 2>/dev/null || true
    as_root iptables -D INPUT ! -i lo -d "$host_ip" -m conntrack --ctdir ORIGINAL -j "$chain" 2>/dev/null || true
    as_root iptables -F "$chain" 2>/dev/null || true; as_root iptables -X "$chain" 2>/dev/null || true
    as_root systemctl disable --now "$chain.service" >/dev/null 2>&1 || true
    as_root rm -f -- "/etc/offline-preops/$chain.sh" "/etc/systemd/system/$chain.service"
    as_root systemctl daemon-reload || true
  fi
  as_root rm -rf -- "$work"
  exit "$rc"
}
trap cleanup EXIT
docker pull docker.io/library/nginx:1.27.2
docker pull docker.io/library/postgres:16.4
docker pull curlimages/curl:8.11.1
docker save -o "$work/nginx.tar" docker.io/library/nginx:1.27.2
docker save -o "$work/postgres.tar" docker.io/library/postgres:16.4
docker network create --subnet 172.30.252.0/24 "$network" >/dev/null
host_ip=$(docker network inspect bridge --format '{{(index .IPAM.Config 0).Gateway}}')
docker_version=$(docker version --format '{{.Server.Version}}')
docker_root=$(docker info --format '{{.DockerRootDir}}')
port=18080; db_service=db
render() {
  if [ -n "${PREOPS_RENDERER:-}" ]; then
    PREOPS_ACCEPTANCE_DEPLOY_BASE="$work" PREOPS_ACCEPTANCE_OS_VERSION="$(. /etc/os-release; echo "$VERSION_ID")" WSLENV=PREOPS_ACCEPTANCE_DEPLOY_BASE:PREOPS_ACCEPTANCE_OS_VERSION \
      "$PREOPS_RENDERER" "$(wslpath -w "$work/$1")" "$(wslpath -w "$work/nginx.tar")" "$host_ip" "$docker_version" "$docker_root" "$2" "$3" "$4" "$(wslpath -w "$work/postgres.tar")" "${5:-}"
  else
    cargo run --quiet --locked --manifest-path "$ROOT/src-tauri/Cargo.toml" --example render_acceptance -- \
      "$work/$1" "$work/nginx.tar" "$host_ip" "$docker_version" "$docker_root" "$2" "$3" "$4" "$work/postgres.tar" "${5:-}"
  fi
}
compose() { docker compose --project-directory "$work/full/stack" -f "$work/full/stack/docker-compose.yml" "$@"; }
sql() { compose exec -T "$db_service" sh -c 'PGPASSWORD="$POSTGRES_PASSWORD" psql -h127.0.0.1 -U "$POSTGRES_USER" -d "$POSTGRES_DB" -tAc "$1"' sh "$1"; }
version_is() { test "$(as_root sed -n "s/^BUILD_ID='\([^']*\)'/\1/p" "$work/full/deployment.env")" = "$1"; }
render full b-one '' "$port"
as_root bash "$work/full/scripts/deploy.sh"
as_root bash "$work/full/scripts/deploy.sh"
as_root bash "$work/full/scripts/ops.sh" status
chain=$(sed -n "s/^CHAIN='\([^']*\)'/\1/p" "$work/full/scripts/apply-firewall.sh")
curl --fail --silent "http://$host_ip:$port/" >/dev/null
sql "CREATE TABLE acceptance (value text NOT NULL); INSERT INTO acceptance VALUES ('before');" >/dev/null
if [ "$SCENARIO" = network ]; then
  as_root bash "$work/full/scripts/apply-firewall.sh" --apply --no-persist
  probe_sources() {
    docker run --rm --network "$network" --ip 172.30.252.10 curlimages/curl:8.11.1 -fsS --connect-timeout 5 --max-time 10 "http://$host_ip:$1/" >/dev/null
    if docker run --rm --network "$network" --ip 172.30.252.11 curlimages/curl:8.11.1 -fsS --connect-timeout 3 --max-time 5 "http://$host_ip:$1/" >/dev/null 2>&1; then echo 'Denied source reached published port' >&2; return 1; fi
  }
  probe_sources "$port"
  as_root bash "$work/full/scripts/apply-firewall.sh" --apply --no-persist
  probe_sources "$port"
  if [ "${PREOPS_ACCEPTANCE_NO_PERSIST:-0}" != 1 ]; then
    as_root bash "$work/full/scripts/apply-firewall.sh" --apply
    as_root systemctl restart "$chain.service"
    probe_sources "$port"
    as_root systemctl restart docker
    as_root bash "$work/full/scripts/ops.sh" status
    probe_sources "$port"
  fi
  render upgrade b-two b-one 18081
  as_root bash "$work/upgrade/scripts/upgrade.sh" --target "$work/full"
  as_root bash "$work/full/scripts/apply-firewall.sh" --apply --no-persist
  probe_sources 18081
  echo 'PASS: source allow/deny, policy reapply, port-changing upgrade'
fi
if [ "$SCENARIO" = operations ]; then
  as_root bash "$work/full/scripts/ops.sh" backup
  snapshot=$(as_root find "$work/backups/proj-acceptance/srv-acceptance" -mindepth 1 -maxdepth 1 -type d | head -n 1)
  as_root test -f "$snapshot/COMPLETE"
  sql "INSERT INTO acceptance VALUES ('after');" >/dev/null
  as_root sh -c 'printf changed > "$1"' sh "$work/full/stack/data/inst-nginx/index.html"
  as_root bash "$work/full/scripts/ops.sh" restore "$snapshot" --yes
  test "$(as_root cat "$work/full/stack/data/inst-nginx/index.html" | tr -d '\n')" != changed
  test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
  render upgrade b-two b-one 18081 rename
  as_root bash "$work/upgrade/scripts/upgrade.sh" --target "$work/full" --dry-run
  as_root bash "$work/upgrade/scripts/upgrade.sh" --target "$work/full"
  db_service=db-renamed
  as_root bash "$work/full/scripts/ops.sh" status
  test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
  version_is b-two
  if as_root bash "$work/full/scripts/deploy.sh"; then echo 'Stale full deploy reran after upgrade' >&2; exit 1; fi
  render fail b-three b-two 18082 rename
  sed -i '0,/image: /s#image:.*#image: unavailable:image#' "$work/fail/stack/docker-compose.yml"
  (cd "$work/fail" && find . -type f ! -name SHA256SUMS -print0 | sort -z | xargs -0 sha256sum > SHA256SUMS)
  if as_root bash "$work/fail/scripts/upgrade.sh" --target "$work/full"; then echo 'Broken upgrade succeeded' >&2; exit 1; fi
  as_root bash "$work/full/scripts/ops.sh" status
  test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
  version_is b-two
  as_root bash "$work/full/scripts/ops.sh" rollback "$snapshot" --yes
  db_service=db
  as_root bash "$work/full/scripts/ops.sh" status
  test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
  version_is b-one
  echo 'PASS: cold backup/restore, renamed PostgreSQL data, upgrade, failure rollback'
fi
if [ "$SCENARIO" = identity ]; then
  pair_tag="preops-acceptance/image:pair"
  docker tag docker.io/library/nginx:1.27.2 "$pair_tag"
  docker save -o "$work/pair-a.tar" "$pair_tag"
  docker tag docker.io/library/postgres:16.4 "$pair_tag"
  docker save -o "$work/pair-b.tar" "$pair_tag"
  if [ -n "${PREOPS_IDENTITY_RENDERER:-}" ]; then
    "$PREOPS_IDENTITY_RENDERER" "$(wslpath -w "$work/pair")" "$(wslpath -w "$work/pair-a.tar")" "$(wslpath -w "$work/pair-b.tar")" "$pair_tag"
  else
    cargo run --quiet --locked --manifest-path "$ROOT/src-tauri/Cargo.toml" --example render_identity_pair -- "$work/pair" "$work/pair-a.tar" "$work/pair-b.tar" "$pair_tag"
  fi
  docker load -i "$work/pair/b.tar" >/dev/null
  docker load -i "$work/pair/a.tar" >/dev/null
  while IFS=$'\t' read -r variant expected ref; do
    container=$(docker run -d --label offlinepreops.project=proj-acceptance --memory=64m --cpus=0.25 --entrypoint /bin/sh "$ref" -c 'sleep 60')
    test "$(docker inspect --format '{{.Image}}' "$container")" = "$expected"
    docker rm -f "$container" >/dev/null
  done < "$work/pair/images.tsv"
  docker image rm "$pair_tag" >/dev/null
  running_id=$(docker inspect --format '{{.Image}}' "$(compose ps -q db)")
  runtime_ref=$(compose config --images db)
  docker tag docker.io/library/nginx:1.27.2 "$runtime_ref"
  as_root bash "$work/full/scripts/ops.sh" backup
  snapshot=$(as_root find "$work/backups/proj-acceptance/srv-acceptance" -mindepth 1 -maxdepth 1 -type d | head -n 1)
  as_root grep -F "$running_id" "$snapshot/service-images.tsv" >/dev/null
  as_root bash "$work/full/scripts/ops.sh" restore "$snapshot" --yes
  test "$(docker inspect --format '{{.Image}}' "$(compose ps -q db)")" = "$running_id"
  test "$(sql 'SELECT value FROM acceptance;' | tr -d '[:space:]')" = before
  echo 'PASS: same-tag archives keep independent runtime identities; container identity survives tag drift and restore'
fi
