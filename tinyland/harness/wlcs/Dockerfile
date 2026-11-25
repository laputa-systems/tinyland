FROM alpine:3.22@sha256:49340b346798875b5592e5d420241f2f9cfba5f47e62ebb1446a1b34734e898f
COPY --from=oci.17166969.xyz/owned-tools@sha256:aaec19e7a44cbc53b40db2d22093600ee888689b89be94da90e5398ed02f1883 /opt/laputa /opt/laputa
ENV PATH=$PATH:/opt/laputa/bin

ARG WLCS_COMMIT=2b385328366de5db19521b9b3eef81861430d984

RUN apk add --no-cache bash boost-dev ca-certificates curl git libgcc \
        musl-dev pkgconf wayland-dev wayland-protocols
# Rust links host proc-macros against -lgcc_s, which only gcc's package symlinks.
RUN ln -s libgcc_s.so.1 /usr/lib/libgcc_s.so \
 && ln -s /opt/laputa/bin/clang /usr/local/bin/cc \
 && ln -s /opt/laputa/bin/clang++ /usr/local/bin/c++
ENV AR=llvm-ar

# Alpine's gtest is built against libstdc++; build it against the owned clang++'s libc++.
ARG GOOGLETEST_VERSION=1.17.0
RUN curl -fsSL "https://github.com/google/googletest/releases/download/v$GOOGLETEST_VERSION/googletest-$GOOGLETEST_VERSION.tar.gz" -o /tmp/googletest.tar.gz \
 && echo "65fab701d9829d38cb77c14acdc431d2108bfdbf8979e40eb8ae567edf10b27c  /tmp/googletest.tar.gz" | sha256sum -c - \
 && tar -xzf /tmp/googletest.tar.gz -C /tmp \
 && cmake -G Ninja -S "/tmp/googletest-$GOOGLETEST_VERSION" -B /tmp/googletest-build -DCMAKE_BUILD_TYPE=Release \
 && cmake --build /tmp/googletest-build --target install \
 && rm -rf /tmp/googletest.tar.gz "/tmp/googletest-$GOOGLETEST_VERSION" /tmp/googletest-build

COPY ack-initial-configure.patch /ack-initial-configure.patch
RUN git init /wlcs-src \
 && git -C /wlcs-src fetch --depth 1 https://github.com/canonical/wlcs "$WLCS_COMMIT" \
 && git -C /wlcs-src checkout --detach FETCH_HEAD \
 && git -C /wlcs-src apply /ack-initial-configure.patch \
 && cmake -G Ninja -S /wlcs-src -B /wlcs-build -DCMAKE_BUILD_TYPE=Debug \
 && cmake --build /wlcs-build --target wlcs

ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTFLAGS="-Ctarget-feature=-crt-static"
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o /tmp/rustup-init.sh \
 && sh /tmp/rustup-init.sh -y --profile minimal --default-toolchain nightly-2026-11-09 \
 && rm /tmp/rustup-init.sh

WORKDIR /src
