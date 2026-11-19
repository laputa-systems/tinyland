TEST_IMAGE := tinyland-tests:local

.PHONY: test test-ci test-image

test: test-image
	docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g \
		-v $(HOME)/.cargo/bin/kache:/usr/local/bin/kache:ro \
		-e RUSTC_WRAPPER=/usr/local/bin/kache $(TEST_IMAGE)

test-ci: test-image
	docker run --rm $(TEST_IMAGE)

test-image:
	docker build -f Dockerfile.test -t $(TEST_IMAGE) .
