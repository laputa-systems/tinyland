#!/bin/sh
set -eu
cd "$(dirname "$0")/.."

ORACLE_SOCK="$PWD/.work/tmp/oracle.sock"
RUST_SOCK="$PWD/.work/tmp/libseat-diff-rust.sock"

if [ ! -x .work/tmp/oracle/seatd ] || [ ! -x .work/tmp/client-oracle/simpletest ]; then
	sh harness/build_oracle.sh
fi
cargo build --offline --locked

run_case() {
	name="$1"
	daemon_bin="$2"
	sock="$3"
	daemon_sock_env="$4"
	rm -f "$sock"
	env SEATD_VTBOUND=0 SEATD_SOCK="$daemon_sock_env" \
		"$daemon_bin" -l silent < /dev/null > /dev/null 2>&1 &
	dpid=$!
	ready=0
	for _ in $(seq 1 50); do
		if [ -S "$sock" ]; then
			ready=1
			break
		fi
		sleep 0.1
	done
	if [ "$ready" != "1" ]; then
		echo "daemon $name never created $sock" >& 2
		kill -TERM "$dpid" 2> /dev/null || true
		wait "$dpid" 2> /dev/null || true
		exit 1
	fi
	set +e
	env SEATD_SOCK="$sock" LIBSEAT_BACKEND=seatd \
		LD_LIBRARY_PATH="$PWD/.work/tmp/client-oracle" \
		timeout 20 .work/tmp/client-oracle/simpletest /dev/null \
		< /dev/null > ".work/tmp/libseat-diff-$name.raw" 2>&1
	code=$?
	set -e
	echo "exit:$code" >> ".work/tmp/libseat-diff-$name.raw"
	kill -TERM "$dpid" 2> /dev/null || true
	wait "$dpid" 2> /dev/null || true
	sed -E 's/0x[0-9a-f]+/0xPTR/g; s/^[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{3} /TIME /' ".work/tmp/libseat-diff-$name.raw" \
		> ".work/tmp/libseat-diff-$name.txt"
}

run_case oracle .work/tmp/oracle/seatd "$ORACLE_SOCK" "$ORACLE_SOCK"
run_case rust "${1:-./target/debug/seatd}" "$RUST_SOCK" "$RUST_SOCK"

if diff -u .work/tmp/libseat-diff-oracle.txt .work/tmp/libseat-diff-rust.txt; then
	echo "LIBSEAT-DIFF-OK"
else
	echo "LIBSEAT-DIFF-MISMATCH" >& 2
	exit 1
fi
