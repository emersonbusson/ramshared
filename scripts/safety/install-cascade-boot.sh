#!/usr/bin/env bash
# Install one already-built, sealed NBD release. The no-argument path is a
# read-only plan; every filesystem or systemd write needs exact version scope.
set -euo pipefail

SOURCE_RELEASE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../.." && pwd -P)
PRODUCT_ROOT=/opt/ramshared
RELEASE_ROOT="$PRODUCT_ROOT/releases"
UNIT_PATH=/etc/systemd/system/ramshared-cascade.service
CURRENT_SELECTOR="$PRODUCT_ROOT/current"
APPROVED_VERSION=
LEGACY_UNIT_APPROVED_HASH=
declare -A INSTALL_MANIFEST_HASHES=()
DESTINATION=
STAGING=
SELECTOR_STAGING=
UNIT_STAGING=
ROLLBACK_UNIT_STAGING=
LEGACY_BACKUP_ROOT=
LEGACY_BACKUP=
LEGACY_BACKUP_STAGING=
ROLLBACK_SELECTOR_STAGING=
PRIOR_SELECTOR_TARGET=
PUBLISHED_DESTINATION=0
UNIT_CREATED=0
LEGACY_UNIT_REPLACED=0
LEGACY_UNIT_RELOAD_REQUIRED=0

refuse() {
  printf 'NBD_INSTALL_STATE=REFUSED\n'
  printf 'NBD_INSTALL_REASON=%s\n' "$1"
  exit 1
}

usage() {
  cat <<'EOF'
Usage:
  install-cascade-boot.sh [--plan]
  install-cascade-boot.sh --approve-nbd-product-install <version>
  install-cascade-boot.sh --approve-nbd-product-install <version> \\
    --approve-legacy-unit-replacement <sha256>

The default is a read-only plan. Approval must name exactly the sealed release
version in this bundle. This source-only slice installs its unit disabled: a
separate scoped lifecycle approval is required before it can activate or
deactivate a cascade.
EOF
}

read_release_version() {
  local version trailing
  [[ -f $SOURCE_RELEASE/RELEASE_VERSION && ! -L $SOURCE_RELEASE/RELEASE_VERSION ]] || refuse RELEASE_VERSION_MISSING
  IFS= read -r version <"$SOURCE_RELEASE/RELEASE_VERSION" || refuse RELEASE_VERSION_MISSING
  trailing=$(sed -n '2p' "$SOURCE_RELEASE/RELEASE_VERSION")
  [[ -z $trailing && $version =~ ^[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || refuse RELEASE_VERSION_INVALID
  RELEASE_VERSION=$version
}

verify_release_tree() {
  local root=$1 manifest line digest relative actual listed required
  [[ -f $root/SHA256SUMS && ! -L $root/SHA256SUMS ]] || refuse RELEASE_MANIFEST_MISSING
  [[ -z $(find "$root" -type l -print -quit) ]] || refuse RELEASE_SYMLINK_FORBIDDEN
  [[ -z $(find "$root" \( -type p -o -type b -o -type c -o -type s \) -print -quit) ]] || refuse RELEASE_NON_REGULAR_OBJECT
  manifest="$root/SHA256SUMS"
  INSTALL_MANIFEST_HASHES=()
  while IFS= read -r line || [[ -n $line ]]; do
    [[ $line =~ ^([[:xdigit:]]{64})\ \ \./([A-Za-z0-9][A-Za-z0-9._/-]*)$ ]] || refuse RELEASE_MANIFEST_FORMAT_INVALID
    digest=${BASH_REMATCH[1],,}
    relative=${BASH_REMATCH[2]}
    [[ $relative != SHA256SUMS && $relative != ../* && $relative != */../* && $relative != *'/..' && $relative != *'//' && $relative != *'/./'* ]] || refuse RELEASE_MANIFEST_PATH_INVALID
    [[ -z ${INSTALL_MANIFEST_HASHES[$relative]+x} ]] || refuse RELEASE_MANIFEST_DUPLICATE
    [[ -f $root/$relative && ! -L $root/$relative ]] || refuse RELEASE_MANIFEST_ENTRY_MISSING
    actual=$(sha256sum -- "$root/$relative" | awk '{print $1}')
    [[ $actual == "$digest" ]] || refuse RELEASE_MANIFEST_HASH_MISMATCH
    INSTALL_MANIFEST_HASHES[$relative]=$digest
  done <"$manifest"
  [[ ${#INSTALL_MANIFEST_HASHES[@]} -gt 0 ]] || refuse RELEASE_MANIFEST_EMPTY
  while IFS= read -r -d '' listed; do
    relative=${listed#"$root"/}
    [[ -n ${INSTALL_MANIFEST_HASHES[$relative]+x} ]] || refuse RELEASE_MANIFEST_INCOMPLETE
  done < <(find "$root" -type f ! -name SHA256SUMS -print0)
  for required in \
    bin/ramshared \
    bin/ramsharedd \
    scripts/safety/install-cascade-boot.sh \
    scripts/safety/nbd-product-preflight.sh \
    scripts/safety/cascade-up.sh \
    scripts/safety/cascade-down.sh \
    scripts/safety/wsl-relay-health.sh \
    scripts/safety/cascade.conf.example \
    systemd/ramshared-cascade.service; do
    [[ -f $root/$required && ! -L $root/$required ]] || refuse RELEASE_LAYOUT_INVALID
  done
  [[ -x $root/bin/ramshared && -x $root/bin/ramsharedd ]] || refuse RELEASE_LAYOUT_INVALID
  (cd "$root" && sha256sum -c --status SHA256SUMS) || refuse RELEASE_MANIFEST_HASH_MISMATCH
}

require_sealed_node() {
  local path=$1 metadata mode
  metadata=$(stat -c '%u:%g:%a' -- "$path" 2>/dev/null || true)
  [[ $metadata =~ ^0:0:([0-7]{3,4})$ ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
  mode=${BASH_REMATCH[1]}
  (( (8#$mode & 0222) == 0 )) || refuse RELEASE_SEAL_OWNER_MODE_INVALID

  if [[ -d $path ]]; then
    [[ $mode == 555 ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
  elif [[ -f $path ]]; then
    if [[ -x $path ]]; then
      [[ $mode == 555 ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
    else
      [[ $mode == 444 ]] || refuse RELEASE_SEAL_OWNER_MODE_INVALID
    fi
  else
    refuse RELEASE_NON_REGULAR_OBJECT
  fi
}

verify_sealed_release_tree() {
  local root=$1 path
  while IFS= read -r -d '' path; do
    require_sealed_node "$path"
  done < <(find "$root" -type d -print0)
  while IFS= read -r -d '' path; do
    require_sealed_node "$path"
  done < <(find "$root" -type f -print0)
}

systemctl_status() {
  local operation=$1 unit=$2 status
  set +e
  systemctl "$operation" --quiet "$unit" >/dev/null 2>&1
  status=$?
  set -e
  printf '%s\n' "$status"
}

is_sha256() {
  [[ $1 =~ ^[[:xdigit:]]{64}$ ]]
}

check_unit_inert() {
  local unit=$1 active enabled
  active=$(systemctl_status is-active "$unit")
  case $active in
    3|4) ;;
    0) refuse PRODUCT_UNIT_ACTIVE ;;
    *) refuse PRODUCT_UNIT_ACTIVITY_UNKNOWN ;;
  esac
  enabled=$(systemctl_status is-enabled "$unit")
  case $enabled in
    1) ;;
    0) refuse PRODUCT_UNIT_ENABLED ;;
    *) refuse PRODUCT_UNIT_ENABLEMENT_UNKNOWN ;;
  esac
}

check_existing_unit_file() {
  local expected_unit=${1:-"$SOURCE_RELEASE/systemd/ramshared-cascade.service"}
  [[ ! -L $UNIT_PATH ]] || refuse PRODUCT_UNIT_CONFLICT
  if [[ -e $UNIT_PATH ]]; then
    [[ -f $UNIT_PATH ]] || refuse PRODUCT_UNIT_CONFLICT
    if ! cmp -s "$expected_unit" "$UNIT_PATH"; then
      [[ -n $LEGACY_UNIT_APPROVED_HASH ]] || refuse LEGACY_UNIT_APPROVAL_MISSING
      is_sha256 "$LEGACY_UNIT_APPROVED_HASH" || refuse LEGACY_UNIT_APPROVAL_INVALID
      [[ $(stat -c '%u:%g:%a' -- "$UNIT_PATH" 2>/dev/null || true) == '0:0:644' ]] || refuse LEGACY_UNIT_METADATA_INVALID
      [[ $(sha256sum -- "$UNIT_PATH" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || refuse LEGACY_UNIT_HASH_MISMATCH
    fi
  fi
}

legacy_unit_replacement_required() {
  [[ -f $UNIT_PATH && ! -L $UNIT_PATH ]] && ! cmp -s "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_PATH"
}

verify_legacy_backup_or_refuse() {
  [[ -n $LEGACY_BACKUP && -f $LEGACY_BACKUP && ! -L $LEGACY_BACKUP ]] || refuse LEGACY_UNIT_BACKUP_CONFLICT
  [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP" 2>/dev/null || true) == '0:0:444' ]] || refuse LEGACY_UNIT_BACKUP_CONFLICT
  [[ $(sha256sum -- "$LEGACY_BACKUP" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || refuse LEGACY_UNIT_BACKUP_HASH_MISMATCH
}

prepare_legacy_backup_root_or_refuse() {
  if [[ -e $LEGACY_BACKUP_ROOT || -L $LEGACY_BACKUP_ROOT ]]; then
    [[ -d $LEGACY_BACKUP_ROOT && ! -L $LEGACY_BACKUP_ROOT ]] || refuse LEGACY_BACKUP_ROOT_INVALID
    [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP_ROOT" 2>/dev/null || true) == '0:0:755' ]] || refuse LEGACY_BACKUP_ROOT_INVALID
  else
    install -d -m 0755 "$LEGACY_BACKUP_ROOT"
  fi
  [[ -d $LEGACY_BACKUP_ROOT && ! -L $LEGACY_BACKUP_ROOT ]] || refuse LEGACY_BACKUP_ROOT_INVALID
  [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP_ROOT" 2>/dev/null || true) == '0:0:755' ]] || refuse LEGACY_BACKUP_ROOT_INVALID
}

backup_legacy_unit_if_required() {
  local observed_hash
  legacy_unit_replacement_required || return 0
  observed_hash=$(sha256sum -- "$UNIT_PATH" | awk '{print $1}')
  [[ $observed_hash == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || refuse LEGACY_UNIT_HASH_MISMATCH
  [[ $(stat -c '%u:%g:%a' -- "$UNIT_PATH" 2>/dev/null || true) == '0:0:644' ]] || refuse LEGACY_UNIT_METADATA_INVALID

  LEGACY_BACKUP_ROOT="$PRODUCT_ROOT/legacy-units"
  LEGACY_BACKUP="$LEGACY_BACKUP_ROOT/ramshared-cascade.service.$observed_hash.bak"
  LEGACY_BACKUP_STAGING="$LEGACY_BACKUP_ROOT/.ramshared-cascade.service.$observed_hash.$$.staging"
  prepare_legacy_backup_root_or_refuse
  if [[ -e $LEGACY_BACKUP || -L $LEGACY_BACKUP ]]; then
    verify_legacy_backup_or_refuse
    return 0
  fi

  install -m 0444 "$UNIT_PATH" "$LEGACY_BACKUP_STAGING"
  chown root:root "$LEGACY_BACKUP_STAGING"
  [[ $(sha256sum -- "$LEGACY_BACKUP_STAGING" | awk '{print $1}') == "$observed_hash" ]] || refuse LEGACY_UNIT_BACKUP_HASH_MISMATCH
  mv -T "$LEGACY_BACKUP_STAGING" "$LEGACY_BACKUP"
  verify_legacy_backup_or_refuse
  # NBD_INSTALL_POST_WRITE_PHASE=legacy-unit-backed-up
}

path_exists_or_link() {
  [[ -e $1 || -L $1 ]]
}

capture_prior_selector() {
  local target resolved version
  if [[ -L $CURRENT_SELECTOR ]]; then
    target=$(readlink -- "$CURRENT_SELECTOR" 2>/dev/null || true)
    [[ $target =~ ^releases/[A-Za-z0-9][A-Za-z0-9._-]{0,127}$ ]] || refuse CURRENT_SELECTOR_INVALID
    version=${target#releases/}
    resolved=$(readlink -f -- "$CURRENT_SELECTOR" 2>/dev/null || true)
    [[ $resolved == "$RELEASE_ROOT/$version" && -d $resolved ]] || refuse CURRENT_SELECTOR_INVALID
    PRIOR_SELECTOR_TARGET=$target
  elif [[ -e $CURRENT_SELECTOR ]]; then
    refuse CURRENT_SELECTOR_CONFLICT
  fi
}

selector_points_to_destination() {
  local resolved
  [[ -n $DESTINATION && -L $CURRENT_SELECTOR ]] || return 1
  resolved=$(readlink -f -- "$CURRENT_SELECTOR" 2>/dev/null || true)
  [[ $resolved == "$DESTINATION" ]]
}

remove_path_if_present() {
  local path=$1
  [[ -n $path ]] || return 0
  path_exists_or_link "$path" || return 0
  rm -rf -- "$path"
}

restore_prior_selector() {
  if [[ -n $PRIOR_SELECTOR_TARGET ]]; then
    path_exists_or_link "$ROLLBACK_SELECTOR_STAGING" && return 1
    ln -s "$PRIOR_SELECTOR_TARGET" "$ROLLBACK_SELECTOR_STAGING" || return 1
    chown -h root:root "$ROLLBACK_SELECTOR_STAGING" || return 1
    mv -Tf "$ROLLBACK_SELECTOR_STAGING" "$CURRENT_SELECTOR" || return 1
    return 0
  fi

  rm -f -- "$CURRENT_SELECTOR"
}

remove_created_unit_if_owned() {
  (( UNIT_CREATED )) || return 0
  if [[ -f $UNIT_PATH ]] && cmp -s "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_PATH"; then
    rm -f -- "$UNIT_PATH"
  fi
  UNIT_CREATED=0
}

restore_legacy_unit_if_replaced() {
  (( LEGACY_UNIT_REPLACED )) || return 0
  [[ -n $LEGACY_BACKUP && -f $LEGACY_BACKUP && ! -L $LEGACY_BACKUP ]] || return 1
  [[ $(stat -c '%u:%g:%a' -- "$LEGACY_BACKUP" 2>/dev/null || true) == '0:0:444' ]] || return 1
  [[ $(sha256sum -- "$LEGACY_BACKUP" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || return 1
  ROLLBACK_UNIT_STAGING="$(dirname -- "$UNIT_PATH")/.ramshared-cascade.rollback.$$"
  install -m 0644 "$LEGACY_BACKUP" "$ROLLBACK_UNIT_STAGING" || return 1
  chown root:root "$ROLLBACK_UNIT_STAGING" || return 1
  [[ $(sha256sum -- "$ROLLBACK_UNIT_STAGING" | awk '{print $1}') == "${LEGACY_UNIT_APPROVED_HASH,,}" ]] || return 1
  mv -Tf "$ROLLBACK_UNIT_STAGING" "$UNIT_PATH" || return 1
  if (( LEGACY_UNIT_RELOAD_REQUIRED )); then
    systemctl daemon-reload || return 1
    LEGACY_UNIT_RELOAD_REQUIRED=0
  fi
  LEGACY_UNIT_REPLACED=0
}

rollback_after_failure() {
  local status=$?
  trap - EXIT
  if (( status != 0 )); then
    set +e
    remove_path_if_present "$STAGING"
    remove_path_if_present "$SELECTOR_STAGING"
    if selector_points_to_destination; then
      restore_prior_selector || printf 'NBD_INSTALL_ROLLBACK=SELECTOR_RESTORE_FAILED\n' >&2
    fi
    restore_legacy_unit_if_replaced || printf 'NBD_INSTALL_ROLLBACK=LEGACY_UNIT_RESTORE_FAILED\n' >&2
    remove_created_unit_if_owned
    remove_path_if_present "$UNIT_STAGING"
    remove_path_if_present "$ROLLBACK_UNIT_STAGING"
    remove_path_if_present "$LEGACY_BACKUP_STAGING"
    if (( PUBLISHED_DESTINATION )) && ! selector_points_to_destination; then
      remove_path_if_present "$DESTINATION"
      PUBLISHED_DESTINATION=0
    fi
    remove_path_if_present "$ROLLBACK_SELECTOR_STAGING"
  fi
  exit "$status"
}

install_unit_if_absent() {
  if path_exists_or_link "$UNIT_PATH"; then
    if ! legacy_unit_replacement_required; then
      check_existing_unit_file "$DESTINATION/systemd/ramshared-cascade.service"
      return
    fi
    backup_legacy_unit_if_required
    verify_legacy_backup_or_refuse
    path_exists_or_link "$UNIT_STAGING" && refuse INSTALL_STAGING_EXISTS
    install -m 0644 "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_STAGING"
    # NBD_INSTALL_POST_WRITE_PHASE=legacy-unit-staged
    chown root:root "$UNIT_STAGING"
    mv -Tf "$UNIT_STAGING" "$UNIT_PATH"
    LEGACY_UNIT_REPLACED=1
    # NBD_INSTALL_POST_WRITE_PHASE=legacy-unit-replaced
    return
  fi

  path_exists_or_link "$UNIT_STAGING" && refuse INSTALL_STAGING_EXISTS
  install -m 0644 "$DESTINATION/systemd/ramshared-cascade.service" "$UNIT_STAGING"
  # NBD_INSTALL_POST_WRITE_PHASE=unit-staged
  ln "$UNIT_STAGING" "$UNIT_PATH" || refuse PRODUCT_UNIT_CONFLICT
  UNIT_CREATED=1
  # NBD_INSTALL_POST_WRITE_PHASE=unit-linked
  rm -f -- "$UNIT_STAGING"
  # NBD_INSTALL_POST_WRITE_PHASE=unit-staging-removed
}

while (($# > 0)); do
  case "$1" in
    --plan)
      shift
      ;;
    --approve-nbd-product-install)
      (($# >= 2)) || refuse APPROVAL_SCOPE_MISSING
      APPROVED_VERSION=$2
      shift 2
      ;;
    --approve-legacy-unit-replacement)
      (($# >= 2)) || refuse LEGACY_UNIT_APPROVAL_MISSING
      LEGACY_UNIT_APPROVED_HASH=$2
      shift 2
      ;;
    --enable)
      refuse BOOT_ENABLE_REQUIRES_LIFECYCLE_APPROVAL
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *) refuse UNSUPPORTED_ARGUMENT ;;
  esac
done

read_release_version
verify_release_tree "$SOURCE_RELEASE"

if [[ -z $APPROVED_VERSION ]]; then
  printf 'NBD_INSTALL_STATE=PLAN\n'
  printf 'NBD_INSTALL_RELEASE=%s\n' "$RELEASE_VERSION"
  printf 'NBD_INSTALL_TARGET=%s\n' "$RELEASE_ROOT/$RELEASE_VERSION"
  printf 'NBD_INSTALL_SELECTOR=%s/current\n' "$PRODUCT_ROOT"
  exit 0
fi

[[ $APPROVED_VERSION == "$RELEASE_VERSION" ]] || refuse APPROVAL_SCOPE_INVALID
[[ -z $LEGACY_UNIT_APPROVED_HASH || $LEGACY_UNIT_APPROVED_HASH =~ ^[[:xdigit:]]{64}$ ]] || refuse LEGACY_UNIT_APPROVAL_INVALID
[[ $(id -u) -eq 0 ]] || refuse ROOT_REQUIRED
command -v systemctl >/dev/null 2>&1 || refuse SYSTEMD_UNAVAILABLE
check_unit_inert ramshared-cascade.service
check_unit_inert ramsharedd.service
check_existing_unit_file
capture_prior_selector

DESTINATION="$RELEASE_ROOT/$RELEASE_VERSION"
[[ ! -e $DESTINATION && ! -L $DESTINATION ]] || refuse RELEASE_VERSION_EXISTS
STAGING="$RELEASE_ROOT/.${RELEASE_VERSION}.staging.$$"
SELECTOR_STAGING="$PRODUCT_ROOT/.current.${RELEASE_VERSION}.$$"
UNIT_STAGING="$(dirname -- "$UNIT_PATH")/.ramshared-cascade.${RELEASE_VERSION}.$$"
ROLLBACK_SELECTOR_STAGING="$PRODUCT_ROOT/.rollback-current.${RELEASE_VERSION}.$$"
[[ ! -e $STAGING && ! -L $STAGING && ! -e $SELECTOR_STAGING && ! -L $SELECTOR_STAGING && ! -e $ROLLBACK_SELECTOR_STAGING && ! -L $ROLLBACK_SELECTOR_STAGING ]] || refuse INSTALL_STAGING_EXISTS

trap rollback_after_failure EXIT
install -d -m 0755 "$PRODUCT_ROOT" "$RELEASE_ROOT"
# NBD_INSTALL_POST_WRITE_PHASE=release-roots-prepared

umask 022
install -d -m 0755 "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-directory-created
cp -a "$SOURCE_RELEASE/." "$STAGING/"
# NBD_INSTALL_POST_WRITE_PHASE=release-copied
chown -R root:root "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-owner-normalized
find "$STAGING" -type d -exec chmod 0555 {} +
# NBD_INSTALL_POST_WRITE_PHASE=staging-directories-sealed
find "$STAGING" -type f -perm /111 -exec chmod 0555 {} +
# NBD_INSTALL_POST_WRITE_PHASE=staging-executables-sealed
find "$STAGING" -type f ! -perm /111 -exec chmod 0444 {} +
# NBD_INSTALL_POST_WRITE_PHASE=staging-files-sealed
verify_release_tree "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-manifest-verified
verify_sealed_release_tree "$STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=staging-seal-verified

# Rename only a new, verified version directory. Existing sealed versions are
# never rewritten, and the selector is the only object replaced atomically.
mv -T "$STAGING" "$DESTINATION"
PUBLISHED_DESTINATION=1
# NBD_INSTALL_POST_WRITE_PHASE=destination-published
install_unit_if_absent
ln -s "releases/$RELEASE_VERSION" "$SELECTOR_STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=selector-staged
chown -h root:root "$SELECTOR_STAGING"
# NBD_INSTALL_POST_WRITE_PHASE=selector-owner-normalized
mv -Tf "$SELECTOR_STAGING" "$CURRENT_SELECTOR"
# NBD_INSTALL_POST_WRITE_PHASE=selector-published
if (( LEGACY_UNIT_REPLACED )); then
  LEGACY_UNIT_RELOAD_REQUIRED=1
fi
systemctl daemon-reload
# NBD_INSTALL_POST_WRITE_PHASE=daemon-reloaded

trap - EXIT
printf 'NBD_INSTALL_STATE=INSTALLED\n'
printf 'NBD_INSTALL_RELEASE=%s\n' "$RELEASE_VERSION"
printf 'NBD_INSTALL_SELECTOR=%s/current\n' "$PRODUCT_ROOT"
printf 'NBD_INSTALL_ENABLED=0\n'
