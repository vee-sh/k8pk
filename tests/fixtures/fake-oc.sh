#!/usr/bin/env sh
# Minimal fake `oc` for CI or local testing. Use: `export K8PK_OC=/abs/path/to/fake-oc.sh`
# or `k8pk --oc /abs/path/to/fake-oc.sh login ...`
set -e
case "$1" in
  version)
    echo "Client Version: fake-oc"
    # accept: oc version --client
    ;;
  login)
    echo "fake-oc: would run oc login"
    exit 1
    ;;
  whoami)
    echo "fake-user"
    ;;
  *)
    echo "fake-oc: unknown $*" >&2
    exit 1
    ;;
esac
