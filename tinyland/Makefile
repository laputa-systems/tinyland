SMOKE_TIMEOUT ?= 180
QEMU ?= qemu-system-x86_64
ARCH ?= x86_64

.PHONY: smoke drm-proof wayland-pixels check test audit

check:
	cargo check --locked

test:
	cargo test --locked

drm-proof:
	bash harness/drm-proof.sh

wayland-pixels:
	bash harness/wayland-pixels.sh

smoke:
	bash harness/smoke.sh

audit:
	bash harness/audit.sh
