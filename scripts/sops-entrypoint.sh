#!/bin/sh
# Runtime-only SOPS loading for a shell-bearing image, then direct exec.
#
# The Dockerfile pins the service binary in ENTRYPOINT after this wrapper and
# leaves CMD empty:
#   ENTRYPOINT ["/usr/local/bin/sops-entrypoint.sh", "/usr/local/bin/<binary>"]
#   CMD []
# so `docker run image --flag` and Kubernetes `args:` append flags to the binary
# instead of replacing it.
#
# Nothing is decrypted at `docker build`; ciphertext and the age identity arrive
# at run time ($SOPS_SECRETS_FILE, $SOPS_AGE_KEY / $SOPS_AGE_KEY_FILE). Values
# stay literal: never source/eval dotenv data or print decryption output.
# Deployments that must have secrets set SOPS_REQUIRE_KEY=1 to fail closed.
set -eu

# Clear inherited export attributes before creating private loader variables.
unset _ORES_SOPS_PRINTENV _ORES_SOPS_PLAINTEXT _ORES_SOPS_NL _ORES_SOPS_LINE \
  _ORES_SOPS_REST _ORES_SOPS_KEY _ORES_SOPS_VALUE _ORES_SOPS_SEEN _ORES_SOPS_IMPORT

# The application never needs the decryption identity. Drop it before handoff
# so a compromised process or crash report cannot inherit reusable key data.
_ores_sops_exec() {
  unset SOPS_AGE_KEY SOPS_AGE_KEY_FILE
  exec "$@"
}

if [ "$#" -eq 0 ] || [ -z "$1" ]; then
  printf '%s\n' 'sops-entrypoint: no command configured' >&2
  exit 64
fi
case "${SOPS_REQUIRE_KEY:-0}" in
  0|1) ;;
  *) printf '%s\n' 'sops-entrypoint: invalid required-key setting' >&2; exit 64 ;;
esac
: "${SOPS_SECRETS_FILE:=/run/secrets/app.env}"
if [ ! -f "$SOPS_SECRETS_FILE" ]; then
  if [ "${SOPS_REQUIRE_KEY:-0}" = 1 ] || [ -e "$SOPS_SECRETS_FILE" ]; then
    printf '%s\n' 'sops-entrypoint: required ciphertext is unavailable or not a regular file' >&2
    exit 1
  fi
  _ores_sops_exec "$@"
fi
if [ -z "${SOPS_AGE_KEY:-}" ] && [ -z "${SOPS_AGE_KEY_FILE:-}" ]; then
  if [ "${SOPS_REQUIRE_KEY:-0}" = 1 ]; then
    printf '%s\n' 'sops-entrypoint: required age identity is unavailable' >&2
    exit 1
  fi
  printf '%s\n' 'sops-entrypoint: optional decryption skipped; no age identity supplied' >&2
  _ores_sops_exec "$@"
fi
command -v sops >/dev/null 2>&1 || {
  printf '%s\n' 'sops-entrypoint: sops binary not in image' >&2; exit 1;
}
# Resolve helpers to absolute paths before any decrypted value is parsed.
_ORES_SOPS_PRINTENV=$(command -v printenv) || {
  printf '%s\n' 'sops-entrypoint: printenv binary not in image' >&2; exit 1;
}
case "$_ORES_SOPS_PRINTENV" in
  /*) ;;
  *) printf '%s\n' 'sops-entrypoint: printenv must resolve to an absolute path' >&2; exit 1 ;;
esac
# --input-type is explicit: the tracked name ends in `.enc`, which sops would
# otherwise parse as JSON.
_ORES_SOPS_PLAINTEXT=$(sops --decrypt --input-type dotenv --output-type dotenv "$SOPS_SECRETS_FILE" 2>/dev/null) || {
  printf '%s\n' 'sops-entrypoint: decryption failed' >&2; exit 1;
}

# Iterate in this shell with parameter expansion, not a pipeline/subshell or a
# plaintext here-document. Split only the first '='; do not reinterpret values.
_ORES_SOPS_NL='
'
_ores_sops_record() {
  case "$_ORES_SOPS_REST" in
    *"$_ORES_SOPS_NL"*)
      _ORES_SOPS_LINE=${_ORES_SOPS_REST%%"$_ORES_SOPS_NL"*}
      _ORES_SOPS_REST=${_ORES_SOPS_REST#*"$_ORES_SOPS_NL"} ;;
    *) _ORES_SOPS_LINE=$_ORES_SOPS_REST; _ORES_SOPS_REST= ;;
  esac
  case "$_ORES_SOPS_LINE" in
    ''|'#'*|sops_*=*) return 1 ;;
    *=*) _ORES_SOPS_KEY=${_ORES_SOPS_LINE%%=*}; _ORES_SOPS_VALUE=${_ORES_SOPS_LINE#*=} ;;
    *) printf '%s\n' 'sops-entrypoint: invalid dotenv record' >&2; exit 1 ;;
  esac
  case "$_ORES_SOPS_KEY" in
    ''|*[!A-Za-z0-9_]*|[0-9]*|_ORES_SOPS_*)
      printf '%s\n' 'sops-entrypoint: invalid or reserved variable name' >&2; exit 1 ;;
  esac
  # Authenticated ciphertext still must not become shell, dynamic-loader, libc
  # or wrapper control. Refuse (fail closed) rather than silently skip, so a
  # misconfigured or compromised secret authority is visible at startup.
  #   shell:   PATH IFS CDPATH ENV BASH_ENV BASH_FUNC_* SHELLOPTS BASHOPTS
  #            GLOBIGNORE PS4 POSIXLY_CORRECT
  #   loader:  LD_* DYLD_*
  #   glibc secure-mode (unsecure-envvars.h): GCONV_PATH GETCONF_DIR HOSTALIASES
  #            LOCALDOMAIN LOCPATH MALLOC_* GLIBC_TUNABLES NIS_PATH NLSPATH
  #            RESOLV_HOST_CONF RES_OPTIONS TMPDIR TZDIR
  #   wrapper: SOPS_* in any letter case (lowercase sops_* metadata is skipped)
  case "$_ORES_SOPS_KEY" in
    PATH|IFS|CDPATH|ENV|BASH_ENV|BASH_FUNC_*|SHELLOPTS|BASHOPTS|GLOBIGNORE|PS4|POSIXLY_CORRECT|\
    LD_*|DYLD_*|\
    GCONV_PATH|GETCONF_DIR|HOSTALIASES|LOCALDOMAIN|LOCPATH|MALLOC_*|GLIBC_TUNABLES|NIS_PATH|NLSPATH|\
    RESOLV_HOST_CONF|RES_OPTIONS|TMPDIR|TZDIR|\
    [Ss][Oo][Pp][Ss]_*)
      printf '%s\n' 'sops-entrypoint: reserved variable name' >&2; exit 1 ;;
  esac
}

# First validate every record and snapshot presence, including empty values.
# No exports occur yet, so imported values cannot affect printenv.
_ORES_SOPS_SEEN='|'
_ORES_SOPS_IMPORT='|'
_ORES_SOPS_REST=$_ORES_SOPS_PLAINTEXT
while [ -n "$_ORES_SOPS_REST" ]; do
  if ! _ores_sops_record; then continue; fi
  case "$_ORES_SOPS_SEEN" in
    *"|$_ORES_SOPS_KEY|"*) printf '%s\n' 'sops-entrypoint: duplicate variable name' >&2; exit 1 ;;
  esac
  _ORES_SOPS_SEEN="$_ORES_SOPS_SEEN$_ORES_SOPS_KEY|"
  if "$_ORES_SOPS_PRINTENV" "$_ORES_SOPS_KEY" >/dev/null 2>&1; then
    :
  else
    case "$?" in
      1) _ORES_SOPS_IMPORT="$_ORES_SOPS_IMPORT$_ORES_SOPS_KEY|" ;;
      *) printf '%s\n' 'sops-entrypoint: environment presence check failed' >&2; exit 1 ;;
    esac
  fi
done

# Apply only unset variables after successful validation, so an orchestrator
# value (even an empty one) wins. No external helper runs after the first export.
_ORES_SOPS_REST=$_ORES_SOPS_PLAINTEXT
while [ -n "$_ORES_SOPS_REST" ]; do
  if ! _ores_sops_record; then continue; fi
  case "$_ORES_SOPS_IMPORT" in
    *"|$_ORES_SOPS_KEY|"*) export "$_ORES_SOPS_KEY=$_ORES_SOPS_VALUE" ;;
  esac
done
unset _ORES_SOPS_PRINTENV _ORES_SOPS_PLAINTEXT _ORES_SOPS_NL _ORES_SOPS_LINE \
  _ORES_SOPS_REST _ORES_SOPS_KEY _ORES_SOPS_VALUE _ORES_SOPS_SEEN _ORES_SOPS_IMPORT
# exec, not `sops exec-env`: the application replaces this shell, keeps PID 1,
# and receives SIGTERM directly.
_ores_sops_exec "$@"
