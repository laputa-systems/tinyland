TEST_IMAGE := tinyland-tests:local

.PHONY: test test-ci lint test-image bench bench-check bench-list

test: test-image
	sh scripts/bench-selftest.sh
	mkdir -p .work/kache-cache
	docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g \
		-v "$(CURDIR)/.work/kache-cache:/root/.cache/kache" $(TEST_IMAGE)

test-ci: test

lint: test-image
	docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g $(TEST_IMAGE) \
		cargo clippy --offline --locked --workspace --all-targets -- -D warnings

test-image:
	docker build -f Dockerfile.test -t $(TEST_IMAGE) .

bench:
	sh scripts/bench.sh $(BENCH_ARGS)

bench-check:
	sh scripts/bench.sh --check $(BENCH_ARGS)

bench-list:
	sh scripts/bench.sh --list
