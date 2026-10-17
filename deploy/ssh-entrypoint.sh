#!/bin/sh
set -eu
mkdir -p /run/sshd
if [ ! -f /host-keys/ssh_host_ed25519_key ]; then
    ssh-keygen -q -t ed25519 -N '' -f /host-keys/ssh_host_ed25519_key
fi
exec /usr/sbin/sshd -D -e -p "${SSH_PORT:-2222}"
