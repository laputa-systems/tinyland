TEST_IMAGE := tinyland-tests:local

.PHONY: test test-ci lint test-image

test: test-image
	mkdir -p .work/kache-cache
	docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g \
		-v "$(CURDIR)/.work/kache-cache:/root/.cache/kache" $(TEST_IMAGE)

test-ci: test

lint: test-image
	docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g $(TEST_IMAGE) \
		cargo clippy --offline --locked --workspace --all-targets -- -D warnings

test-image:
	docker build -f Dockerfile.test -t $(TEST_IMAGE) .
