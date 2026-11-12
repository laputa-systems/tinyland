.PHONY: test

test:
	docker build -f Dockerfile.test -t tinyland-tests:local .
	docker run --rm --init --cpus 4 --memory 8g --memory-swap 8g \
		-v $(HOME)/.cargo/bin/kache:/usr/local/bin/kache:ro \
		-e RUSTC_WRAPPER=/usr/local/bin/kache tinyland-tests:local
