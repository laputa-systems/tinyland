.PHONY: check test audit proof seat

check:
	cargo check --locked -p tinyland

test:
	cargo test --locked -p tinyland

audit:
	bash harness/audit.sh

proof:
	/bin/bash harness/build-guest.sh
	/bin/bash harness/seat-proof.sh

seat: proof
