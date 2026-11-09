#!/usr/bin/env bash
# tpt against a real forward proxy (squid), which VTest2 cannot be.
#
# Usage (from the repository root, on a host with podman, openssl, curl):
#   cargo build && tests/proxy/squid.sh
# Inside a flatpak sandbox (podman only on the host):
#   flatpak-spawn --host tests/proxy/squid.sh
#
#   curl --connect-to backend.test:<backend>:127.0.0.1:<tpt> https://backend.test:<backend>/
#     │  ClientHello, SNI = backend.test
#     ▼
#   tpt (one instance, one server per scenario)
#     │  CONNECT backend.test:<backend>
#     ▼
#   squid (podman, host network, backend.test → 127.0.0.1)
#     ▼
#   openssl s_server -www (certificate for backend.test from the test CA)
#
# Environment variables:
#   TPT_BIN      tpt binary   (default: target/debug/tls-proxy-tunnel)
#   SQUID_IMAGE  squid image built from tests/proxy/Containerfile
#                (default: localhost/tpt-proxy-test-squid)
#   PODMAN       podman binary (default: podman)
#
# Checked:
#   1. tunnel:        curl through tpt and squid reaches the backend
#   2. auth ok:       tpt sends Proxy-Authorization from $TPT_PROXY_AUTH
#   3. auth missing:  squid answers 407, tpt closes, tpt keeps running
#   4. bump with CA:  squid re-signs, curl trusts squid's CA, squid sees GET
#   5. bump w/o CA:   curl rejects squid's certificate
#   6. unknown SNI:   default ban, squid sees no CONNECT

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
TPT_BIN="${TPT_BIN:-${REPO_ROOT}/target/debug/tls-proxy-tunnel}"
SQUID_IMAGE="${SQUID_IMAGE:-localhost/tpt-proxy-test-squid}"
PODMAN="${PODMAN:-podman}"
HOST=backend.test
PROXY_USER=tpt
PROXY_PASS=secret

for tool in "${PODMAN}" openssl curl; do
    command -v "${tool}" > /dev/null || { echo "ERROR: ${tool} not found" >&2; exit 1; }
done
[[ -x "${TPT_BIN}" ]] || { echo "ERROR: ${TPT_BIN} not found, run cargo build" >&2; exit 1; }

WORK="$(mktemp -d "${XDG_RUNTIME_DIR:-/tmp}/tpt-proxy.XXXXXX")"
SQUID="tpt-proxy-test-$$"
BACKEND_PID=""
TPT_PID=""
cleanup() {
    for pid in "${TPT_PID}" "${BACKEND_PID}"; do
        [[ -n "${pid}" ]] || continue
        kill "${pid}" 2> /dev/null || true
        wait "${pid}" 2> /dev/null || true
    done
    "${PODMAN}" rm -f "${SQUID}" > /dev/null 2>&1 || true
    rm -rf "${WORK}"
}
trap cleanup EXIT

fail() {
    echo "FAIL: $*" >&2
    echo "--- squid" >&2
    "${PODMAN}" logs "${SQUID}" 2>&1 | tail -20 >&2 || true
    for log in "${WORK}"/*.log; do
        [[ -e "${log}" ]] || continue
        echo "--- ${log##*/}" >&2
        tail -10 "${log}" >&2
    done
    exit 1
}

# A port nobody listens on. Below 32768: a port in the ephemeral range can be
# held by a TIME-WAIT of some outgoing connection, which blocks bind().
free_port() {
    local p
    for _ in $(seq 1 50); do
        p=$((20000 + RANDOM % 12000)) # below the ephemeral range (32768+)
        if ! (exec 3<> "/dev/tcp/127.0.0.1/${p}") 2> /dev/null; then
            echo "${p}"
            return
        fi
    done
    return 1
}

wait_port() {
    for _ in $(seq 1 100); do
        (exec 3<> "/dev/tcp/127.0.0.1/$1") 2> /dev/null && return 0
        sleep 0.2
    done
    return 1
}

# Lines in squid's access log that match a regex.
squid_log() { "${PODMAN}" logs "${SQUID}" 2> /dev/null | grep -c -E "$1" || true; }

# fetch TPT_PORT [CURL ARGS…]: GET https://backend.test:BACKEND_PORT/, but
# connect to tpt (like DNS pointing the host at tpt). The URL keeps the
# backend port: a bumping squid takes its target from the Host header.
fetch() {
    local port=$1
    shift
    curl -sS --max-time 10 --connect-to "${HOST}:${BACKEND_PORT}:127.0.0.1:${port}" \
        "$@" "https://${HOST}:${BACKEND_PORT}/"
}

make_ca() {
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
        -keyout "$1.key" -out "$1.crt" -days 1 -nodes -subj "/CN=$2" \
        -addext "basicConstraints=critical,CA:true" \
        -addext "keyUsage=critical,keyCertSign,cRLSign" 2> /dev/null
}

echo "==> Certificates: test CA (backend ${HOST}), squid's MITM CA"
(
    cd "${WORK}"
    make_ca ca test-ca
    printf "subjectAltName=DNS:%s\nextendedKeyUsage=serverAuth\n" "${HOST}" > backend.cnf
    openssl req -newkey ec -pkeyopt ec_paramgen_curve:P-256 \
        -keyout backend.key -out backend.csr -nodes -subj "/CN=${HOST}" 2> /dev/null
    openssl x509 -req -in backend.csr -CA ca.crt -CAkey ca.key \
        -CAcreateserial -out backend.crt -days 1 -extfile backend.cnf 2> /dev/null
    make_ca mitm-ca test-mitm-ca
)

echo "==> Backend: openssl s_server -www"
BACKEND_PORT="$(free_port)" || fail "no free port"
openssl s_server -www -quiet -accept "127.0.0.1:${BACKEND_PORT}" \
    -cert "${WORK}/backend.crt" -key "${WORK}/backend.key" \
    > "${WORK}/backend.log" 2>&1 < /dev/null &
BACKEND_PID=$!
wait_port "${BACKEND_PORT}" || fail "backend does not listen on ${BACKEND_PORT}"

echo "==> squid (${SQUID_IMAGE} from tests/proxy/Containerfile)"
"${PODMAN}" build -q -t "${SQUID_IMAGE}" -f "${REPO_ROOT}/tests/proxy/Containerfile" \
    "${REPO_ROOT}/tests/proxy" > /dev/null
TUNNEL_PORT="$(free_port)" || fail "no free port"
AUTH_PORT="$(free_port)" || fail "no free port"
BUMP_PORT="$(free_port)" || fail "no free port"
# squid runs as its own user: the files it reads are world-readable.
mkdir -m 755 "${WORK}/squid"
cp "${WORK}/ca.crt" "${WORK}/mitm-ca.crt" "${WORK}/mitm-ca.key" "${WORK}/squid/"
echo "${PROXY_USER}:$(openssl passwd -apr1 "${PROXY_PASS}")" > "${WORK}/squid/passwd"
sed -e "s|@TUNNEL_PORT@|${TUNNEL_PORT}|" -e "s|@AUTH_PORT@|${AUTH_PORT}|" \
    -e "s|@BUMP_PORT@|${BUMP_PORT}|" \
    "${REPO_ROOT}/tests/proxy/squid.conf" > "${WORK}/squid/squid.conf"
chmod 644 "${WORK}/squid/"*
"${PODMAN}" run -d --name "${SQUID}" --network host --add-host "${HOST}:127.0.0.1" \
    -v "${WORK}/squid:/etc/squid/test:ro,Z" "${SQUID_IMAGE}" > /dev/null
for port in "${TUNNEL_PORT}" "${AUTH_PORT}" "${BUMP_PORT}"; do
    wait_port "${port}" || fail "squid does not listen on ${port}"
done

echo "==> tpt: one server per scenario"
T_TUNNEL="$(free_port)" || fail "no free port"
T_AUTH="$(free_port)" || fail "no free port"
T_NOAUTH="$(free_port)" || fail "no free port"
T_BUMP="$(free_port)" || fail "no free port"
server() { # NAME PORT UPSTREAM [HEADERS-YAML]
    cat << EOF
  $1:
    listen: ["127.0.0.1:$2"]
    tls: true
    sni:
      ${HOST}: $3
    default: ban
    via:
      use_sni_as_target: true
      target_port: ${BACKEND_PORT}
      connect_timeout: 5s
${4:-}
EOF
}
{
    echo "version: 1"
    echo "log: debug"
    echo "servers:"
    server tunnel "${T_TUNNEL}" squid_tunnel
    server auth "${T_AUTH}" squid_auth "      headers:
        Proxy-Authorization: \"Basic \$TPT_PROXY_AUTH\""
    server noauth "${T_NOAUTH}" squid_auth
    server bump "${T_BUMP}" squid_bump
    echo "upstream:"
    echo "  squid_tunnel: \"tcp://127.0.0.1:${TUNNEL_PORT}\""
    echo "  squid_auth: \"tcp://127.0.0.1:${AUTH_PORT}\""
    echo "  squid_bump: \"tcp://127.0.0.1:${BUMP_PORT}\""
} > "${WORK}/tpt.yaml"
TPT_PROXY_AUTH="$(printf '%s:%s' "${PROXY_USER}" "${PROXY_PASS}" | openssl base64 -A)" \
    "${TPT_BIN}" --config "${WORK}/tpt.yaml" > "${WORK}/tpt.log" 2>&1 < /dev/null &
TPT_PID=$!
for port in "${T_TUNNEL}" "${T_AUTH}" "${T_NOAUTH}" "${T_BUMP}"; do
    wait_port "${port}" || fail "tpt does not listen on ${port}"
done

CONNECT_RE="CONNECT ${HOST}:${BACKEND_PORT} "

echo "==> 1. Tunnel"
fetch "${T_TUNNEL}" --cacert "${WORK}/ca.crt" > "${WORK}/1.out" 2>&1 || fail "curl through the tunnel"
grep -q "s_server" "${WORK}/1.out" || fail "no backend answer through the tunnel"
test "$(squid_log "TCP_TUNNEL/200 .* ${CONNECT_RE}")" -eq 1 || fail "squid logged no tunnel"

echo "==> 2. Auth: Proxy-Authorization from \$TPT_PROXY_AUTH"
fetch "${T_AUTH}" --cacert "${WORK}/ca.crt" > "${WORK}/2.out" 2>&1 || fail "curl through the auth port"
grep -q "s_server" "${WORK}/2.out" || fail "no backend answer through the auth port"
test "$(squid_log "TCP_TUNNEL/200 .* ${CONNECT_RE}${PROXY_USER} ")" -eq 1 \
    || fail "squid logged no authenticated tunnel"

echo "==> 3. Auth missing: 407, tpt closes and keeps running"
if fetch "${T_NOAUTH}" --cacert "${WORK}/ca.crt" > "${WORK}/3.out" 2>&1; then
    fail "curl succeeded without proxy credentials"
fi
test "$(squid_log "TCP_DENIED/407 .* ${CONNECT_RE}")" -ge 1 || fail "squid logged no 407"
kill -0 "${TPT_PID}" 2> /dev/null || fail "tpt died after 407"

echo "==> 4. Man in the middle, client trusts squid's CA"
fetch "${T_BUMP}" --cacert "${WORK}/mitm-ca.crt" > "${WORK}/4.out" 2>&1 || fail "curl through the man in the middle"
grep -q "s_server" "${WORK}/4.out" || fail "no backend answer through the man in the middle"
test "$(squid_log "/200 .* GET https://${HOST}:${BACKEND_PORT}/")" -eq 1 \
    || fail "squid did not see the request in clear"

echo "==> 5. Man in the middle, client trusts only the backend's CA"
if fetch "${T_BUMP}" --cacert "${WORK}/ca.crt" > "${WORK}/5.out" 2>&1; then
    fail "curl trusted squid's certificate without its CA"
fi
grep -qi "certificate" "${WORK}/5.out" || fail "no certificate error: $(cat "${WORK}/5.out")"

echo "==> 6. Unknown SNI: default ban, nothing reaches squid"
before="$(squid_log "CONNECT ")"
if curl -sS --max-time 10 -k --connect-to "other.test:${BACKEND_PORT}:127.0.0.1:${T_TUNNEL}" \
    "https://other.test:${BACKEND_PORT}/" > "${WORK}/6.out" 2>&1; then
    fail "curl succeeded with an unknown SNI"
fi
test "$(squid_log "CONNECT ")" -eq "${before}" || fail "unknown SNI reached squid"
test "$(squid_log "other.test")" -eq 0 || fail "squid saw other.test"
kill -0 "${TPT_PID}" 2> /dev/null || fail "tpt died"

echo "PASS: tpt through squid (tunnel, auth, 407, man in the middle, ban)"
